use ndarray::{Array2, Array1, OwnedRepr, Array, Dim, ArrayBase};

use crate::{Model, sync::LayerManager};

use self::model::Layer;
use std::{fmt, sync::{Arc, mpsc::channel}, mem::replace, thread, intrinsics::transmute};

pub mod model;
pub(crate) mod builder;
pub(crate) mod solver_v1;

/// Represents the 'spike' that stimulates a neuron in a spiking neural network.
///  
/// The parameter _'ts'_ stands for 'Time of the Spike' and represents the time when the spike occurs
/// while the parameter _'neuron_id'_ stands to

// TODO Provare Efficienza una tupla al posto di una struct
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Spike {
    pub ts: u128,
    pub neuron_id: usize
}

impl Spike {
    //Di interfaccia
    pub fn new(ts: u128, neuron_id: usize) -> Spike{
        Spike {
            ts,
            neuron_id
        }
    }

    //Di interfaccia
    /// Create an array of spikes for a single neuron, given its ID.
    /// 
    /// You can also give an unordered ts array as shown in the following example.
    /// # Example of Usage
    /// 
    /// ```
    ///  let spikes_neuron_2 = [11, 9, 23, 43, 42].to_vec();
    ///  let spike_vec_for_neuron_2 = Spike::spike_vec_for(neuron_id: 2, ts_vec: spikes_neuron_2 );
    /// 
    /// ```
    pub fn spike_vec_for(neuron_id: usize, ts_vec: Vec<u128>) -> Vec<Spike> {

        let mut spike_vec : Vec<Spike> = Vec::with_capacity(ts_vec.len());
        
        //Creating the Spikes array for a single Neuron
        for ts in ts_vec.into_iter() {
            spike_vec.push(Spike::new(ts, neuron_id));
        }

        //Order the ts vector
        spike_vec.sort();

        spike_vec
    }


    /// Create an ordered array starting from all the spikes sent to the NN.
    /// It takes a Matrix where each row i-th represents an array of spike for neuron i-th
    /// then a single Vec is created. Eventually the array is sorted
    /// 
    /// # Example
    /// ```
    ///  use crate::nn::Spike;
    /// 
    ///  let spikes_neuron_1 = [11, 9, 23, 43, 42].to_vec();
    ///  let spike_vec_for_neuron_1 = Spike::spike_vec_for(2, spikes_neuron_1 );
    ///  
    ///  let spikes_neuron_2 = [1, 29, 3, 11, 22].to_vec();
    ///  let spike_vec_for_neuron_2 = Spike::spike_vec_for(2, spikes_neuron_2 );
    ///  
    ///  let mut spikes: Vec<Vec<Spike>> = Vec::new();
    ///  spikes.push(spike_vec_for_neuron_1);
    ///  spikes.push(spike_vec_for_neuron_2);
    ///  
    ///  let sorted_spike_array_for_nn: Vec<Spike> = Spike::create_terminal_vec(spikes)
    /// 
    /// ```
    pub fn create_terminal_vec(spikes: Vec<Vec<Spike>>) -> Vec<Spike> {
        let mut res: Vec<Spike> = Vec::new();

        for line in spikes {
            for spike in line {
                res.push(spike);
            }
        }
        res.sort(); //ascending
        //TODO cancellare? res.sort_by(|a, b| a.ts.partial_cmp(&b.ts));
    
        res
    }
}

impl fmt::Display for Spike {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.ts, self.neuron_id)
    }
}

/// The Neural Network itself.
/// 
/// This organizes `Neuron`s into consecutive layers, each constituted of some amount of `Neuron`s.
/// `Neuron`s of the same or consecutive layers are connected by a weighted `Synapse`.
/// 
/// A neural network is stimulated by `Spike`s applied to the `Neuron`s of the entry layer.
#[derive(Clone)]
pub struct NN<M: Model> {
    /// Input weight for each of the `Neuron`s in the entry layer
    input_weights: Vec<f64>,
    /// All the layers of the neural network. Every layer contains the list of its `Neuron`s and
    /// a square `Matrix` for the intra-layer weights.
    layers: Vec<Layer<M>>,
    /// Vec of `Synapse` meshes between each consecutive pair of layers
    synapses: Vec<Array2<f64>>
}

// I need to explicitly request RefInto<SolverVars> for Neuron because of a limitation in the Rust compiler with respect
// to implied bounds. See: https://users.rust-lang.org/t/hrtb-in-trait-definition-for-associated-types/78687
impl<M: Model> NN<M> where for<'a> &'a M::Neuron: Into<M::SolverVars> {
    /// Solve the neural network stimulated by the provided spikes.
    /// This function returns the list of spikes generated by the last layer, sorted by timestamp.
    pub fn solve(&self, spikes: Vec<Spike>) -> Vec<Spike> {
        // These will be respectively the first layer's sender and the last layer's receiver
        let (sender, mut receiver) = channel();
        
        for (i, (neurons, synapses_intra)) in self.layers.iter().skip(1).enumerate() {
            let (layer_sender, layer_receiver) = channel();
            
            // Create the LayerManager for this layer
            let (mngr, tokens) = LayerManager::new(
                neurons.len(),
                replace(&mut receiver, layer_receiver),
                layer_sender,
                &self.synapses[i],
                synapses_intra
            );

            // We're gonna share mngr with multiple threads. Since I know the threads will live less than
            // the lifetime 'a of the LayerManager<'a>, I can use some unsafe to allow this.
            let mngr = Arc::new(unsafe { transmute::<_, LayerManager<'_>>(mngr) });

            // Create a new thread for each neuron of the layer
            for (neuron, token) in neurons.iter().zip(tokens.into_iter()) {
                // Same as for mngr, use an anonymous lifetime to pass to the thread
                let neuron = unsafe { transmute::<_, &M::Neuron>(neuron) };
                let mngr = Arc::clone(&mngr);

                thread::spawn(move || {
                    let mut solver_vars: M::SolverVars = neuron.into();
                    
                    while let Some((ts, weighted_input_val)) = mngr.next(&token) {
                        let output = M::handle_spike(neuron, &mut solver_vars, weighted_input_val, ts);
                        let spiked = output > 0.5; // TODO: do we really want this?
                        mngr.commit(&token, spiked, output);
                    }
                });
            }
        }

        // Handle first layer
        {
            // Note that any nn will have at least one layer. Always.
            // Generate SolverVars for each neuron
            let mut layer = self.layers[0].0.iter()
                .map(|neuron| (neuron, neuron.into()))
                .collect::<Vec<(_, M::SolverVars)>>();

            // Used when necessary to inject intra-spikes into layer
            let mut intra_inputs: Option<Array1<f64>> = None;
            let mut inputs = spikes.into_iter();
            let mut cur_ts = 0;

            loop {
                // TODO: for now forbid multiple simultaneous input spikes
                if let Some(intra_arr) = intra_inputs.take() {
                    let mut spiked = false;
                    let output = Array2::from_shape_fn(
                        (1, layer.len()),
                        |(_, i)| {
                            let output = M::handle_spike(layer[i].0, &mut layer[i].1, intra_arr[i], cur_ts);
                            if output > 0.5 { spiked = true; }
                            output
                        }
                    );
                    if spiked {
                        sender.send((cur_ts, output.clone())).unwrap();
                        intra_inputs = Some((output.dot(&self.layers[0].1)).row(0).to_owned());
                    }
                } else {
                    // Get the next spike from the input spikes
                    match inputs.next() {
                        Some(Spike{ neuron_id, ts }) => {
                            cur_ts = ts;
                            
                            // Apply the spike
                            let output = M::handle_spike(layer[neuron_id].0, &mut layer[neuron_id].1, self.input_weights[neuron_id], ts);
                            if output > 0.5 { // TODO: do we really want this?
                                // Neuron spiked, send spike to next layer and enqueue intra-spikes
                                sender.send((ts, Array2::from_shape_fn((1, layer.len()), |(_, i)| if i == neuron_id { output } else { 0.0 }))).unwrap();
                                intra_inputs = Some(self.layers[0].1.row(neuron_id).to_owned() * output);
                            }
                        },
                        None => break
                    }
                }
            }
        }

        // Drop the first sender.
        // This will cause a chain reaction that will ultimately lead to the last receiver being closed.
        drop(sender);

        // Read spikes from last layer and convert to proper format for output
        receiver.into_iter().flat_map(|(ts, arr)| {
            arr.into_iter()
                .enumerate()
                .filter(|(_, v)| *v > 0.5) // TODO: Do we really want this?
                .map(move |(i, _)| Spike {neuron_id: i, ts})
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{nn::Spike, NNBuilder, LeakyIntegrateFire, LifNeuronConfig};
    
    #[test]
    fn test_spike_vec_for(){
        
    }

    #[test]
    fn test_sort_spike(){

        let ts1 = [0, 1, 4, 5].to_vec();
        let ts2 = [0, 3, 6, 7].to_vec();
        let mut multiple_spike_vec : Vec<Vec<Spike>> = Vec::new();
        
        let spike1 = Spike::spike_vec_for(1, ts1);
        let spike2 = Spike::spike_vec_for(2, ts2);

        multiple_spike_vec.push(spike1);
        multiple_spike_vec.push(spike2);

        let input_vec = Spike::create_terminal_vec(multiple_spike_vec);

        for el in input_vec.iter(){
            println!("{:?}", el);
        }
    }

    #[test]
    fn test_create_terminal_vec(){

        let spikes_neuron_1 = [11, 9, 23, 43, 42].to_vec();
        let spike_vec_for_neuron_1 = Spike::spike_vec_for(1, spikes_neuron_1 );
        
        let spikes_neuron_2 = [1, 29, 3, 11, 22].to_vec();
        let spike_vec_for_neuron_2 = Spike::spike_vec_for(2, spikes_neuron_2 );
        
        let spikes: Vec<Vec<Spike>> = [spike_vec_for_neuron_1, 
                                           spike_vec_for_neuron_2].to_vec();
        
        let sorted_spike_array_for_nn: Vec<Spike> = Spike::create_terminal_vec(spikes);
        println!("{:?}", sorted_spike_array_for_nn);
    }

    #[test]
    fn test_solve_nn() {
        // Create a stupidly simple NN
        let cfg = LifNeuronConfig::new(1.0, 0.5, 2.0, 1.0);
        let nn = NNBuilder::<LeakyIntegrateFire, _>::new()
            .layer(
                [From::from(&cfg), From::from(&cfg)],
                [1.2, 2.3],
                [[0.0, -0.8], [-0.6, 0.0]]
            )
            .layer(
                [From::from(&cfg), From::from(&cfg), From::from(&cfg)],
                [
                    [1.5, 1.2, 1.6],
                    [1.2, 1.4, 1.4]
                ],
                [
                    [0.0, -0.4, -0.3],
                    [-0.5, 0.0, -0.5],
                    [-0.8, -0.4, 0.0]
                ]
            )
            .build();

        // Create some input spikes
        let spikes = Spike::create_terminal_vec(vec![
            Spike::spike_vec_for(0, vec![0, 1, 4, 6, 8, 10, 14]),
            Spike::spike_vec_for(1, vec![2, 3, 5, 7, 11, 20]) // No simultaneous spikes
        ]);

        let output = nn.solve(spikes);
        println!("{:?}", output);
    }
}

