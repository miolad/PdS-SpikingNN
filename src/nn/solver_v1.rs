use crate::{nn::{Spike, NN}, Model};
use ndarray::{Array2, OwnedRepr, ArrayBase, Dim};


/// This struct is used to manage the input spikes given a NN,
/// to generate the output spikes.
pub struct Solver<M: Model>{
    input_spikes: Vec<Spike>,
    network: NN<M>,
}

/// This struct is used to manage the internal vars of a Neuron
struct SimulatedNeuron<M: Model> { 
    vars: M::SolverVars
}

impl<M: Model> SimulatedNeuron<M> 
where for <'a> &'a M::Neuron: Into<M::SolverVars> {

    ///Build a new instance of [SimulatedNeuron] given a [Neuron].
    /// 
    /// The [SimulatedNeuron] initially contains the same internals vars of the reference [Neuron].
    pub fn new(neuron: &M::Neuron) -> SimulatedNeuron<M>{
        SimulatedNeuron { vars: neuron.into()}
    }
}

/// This struct is used to manage the internal vars of a NN
struct SimulatedNN<M: Model> {
    layers: Vec<Vec<SimulatedNeuron<M>>>
}

impl<M: Model> SimulatedNN<M> {
    ///Build a new instance of [SimulatedNN].
    fn new() -> Self{
        Self { 
            ///Vector that contains all the layers inside the NN.
            layers: Vec::new(),
        }
    }

    ///This method adds a new layer to [SimulatedNN].
    fn add_layer(&mut self, layer: Vec<SimulatedNeuron<M>>){
        self.layers.push(layer);
    }
}

impl<M: Model> Solver<M> 
where for <'a> &'a M::Neuron: Into<M::SolverVars> {
    
    ///Build a new instance of [Solver], needed to solve the network given the input spikes vector.
    pub fn new(input_spikes: Vec<Spike>, network: NN<M>) -> Solver<M> {
        Solver { 
            input_spikes, 
            network 
        }
    }

    /// Each spike of the input_spike vec is sent to the corresponding neuron 
    /// of the input layer, one by one.
    pub fn solve(&mut self) -> Vec<Vec<u128>>{

        //Inizialization of Neuron variables
        let mut sim_network = Self::init_neuron_vars(&(self.network));
        let mut nn_output: Vec<Vec<u128>> = Vec::new();
        

        //Iteration over the spikes input vector
        for spike in self.input_spikes.iter() {

            //Input dimension taken from layers 0 (1st Layer)
            let dim_input = self.network.layers[0].neurons.len();

            //Spike array creation, involved in a multiplication with the first (diagonal) weight matrix (input matrix).
            let spike_array = single_spike_to_vec(spike.neuron_id, dim_input);

            //Propagation of spikes inside the network
            let res = Solver::infer_spike_vec(&self.network, &mut sim_network, spike_array, spike.ts);
        
            nn_output.push(res);
        }
        // nn_output

        // miolad: changed return format to be compatible with parallel solver
        let mut output = vec![vec![]; self.network.layers.last().unwrap().neurons.len()];

        for spike in nn_output {
            for (neuron_id, ts) in spike.into_iter().enumerate().filter(|(_, v)| *v != u128::MAX) {
                output[neuron_id].push(ts);
            }
        }
        output
    }

    /// _*--> (Internal Use Only)*_
    /// 
    /// Create a temporary NN, parallel to the real one passed as a parameter
    /// 
    /// This new NN will contain only variables like v_mem, ts_old etc
    fn init_neuron_vars(network: & NN<M>) -> SimulatedNN<M> {
        
        let mut sim_nn = SimulatedNN::new();
        let mut sim_neuron: SimulatedNeuron<M>;
        let mut sim_layer: Vec<SimulatedNeuron<M>>;

        for layer in network.layers.iter() {
            sim_layer = Vec::with_capacity(layer.neurons.len());

            for neuron in layer.neurons.iter() {
                sim_neuron = SimulatedNeuron::new(neuron);
                sim_layer.push(sim_neuron);
            }
            sim_nn.add_layer(sim_layer);
        }
        sim_nn
    }

    /// Propagate Spikes inside the network and then create a Vec of spike
    fn infer_spike_vec(
                network: & NN<M>, 
                sim_network: &mut SimulatedNN<M>, 
                spike_vec: ArrayBase<OwnedRepr<f64>, 
                Dim<[usize; 2]>>, 
                ts: u128) -> Vec<u128> {

        //Creation of the output spikes vector
        let mut out_spikes: Vec<u128> = Vec::new();

        //Creation of output vector containing the spikes generated by neurons
        let mut output_vec: Vec<f64> = Vec::new();

        //Creation of vector that contains the variables of the i-th simulatedLayer
        let mut neuron_vars: &mut Vec<SimulatedNeuron<M>> ;
        
        let mut current_spike_vec = spike_vec;

        //per ogni layer della rete prende il layer e il rispettivo layer simulato (Con le vars)
        //crea i vettori di support per l'input e per l'output del layer i-esimo 

        // We compute for each neuron inside the layer its output (if it generates a spike or not)
        for (layer, sim_layer) in network.layers.iter().zip(&mut sim_network.layers){
            
            //Variables of i-th layer
            neuron_vars = sim_layer;

            /* 
            // qui current_spike_vec è qualcosa del tipo [0 1 0 0 0]' oppure  [ 0 1 0 0 1] ed
            // è generato dal layer precedente.
            //creo il vettore dei valori di input per i neuroni ricevuti dal layer precedente, tramite prodotto vec x mat
            
            //println!("current_spike_vec  {}\n\n STTTOOOOP", current_spike_vec);
            //println!("layer.input_weights  {}\n\n STTTOOOOP", layer.input_weights);
            */

            //We use `current_spike_vec` (vector containing the spikes generated by the previous layer)
            //to compute the weighted spikes received to the current layer, we use a dot product.
            let weighted_input_val = current_spike_vec.dot(&layer.input_weights);

            // per ogni neurone, attivo la funzione handle_spike coi suoi parametri e le sue variabili, 
            // prese dai vettori inizializzati precedentemente
            // raccolgo l'output nel vettore
            // Gestisce gli input dal layer precedente

            // For each neuron in the layer, we use the `handle_spike` function given the neuron parameters and variables and 
            // the previously computed input. We can obtain a spike (`1`) or not (`0`) 
            for (i, neuron) in layer.neurons.iter().enumerate(){
                
                let res = M::handle_spike(neuron, 
                    &mut neuron_vars[i].vars, 
                    weighted_input_val[[0,i]], 
                    ts);
                output_vec.push(res);
            }

            //We update the `current_spike_vec` with the obtained results
            current_spike_vec =  Array2::from_shape_vec([1, output_vec.len()], output_vec.clone()).unwrap();

            /*una volta che il layer ha elaborato l'input, bisogna simulare 
            le spike che arrivano ai neuroni dello stesso strato usando il nuovo current_spike_vec aggiornato*/

            //Svuota il vettore che tiene le spike generate dai vari neuroni
            // e lo prepara per la prossima iterazione sui layer

            //TODO fare un check che sia l'ultimo layer cosi non si chiama per ogni layer ma solo sull'ultimo
            out_spikes = to_u128_vec(&output_vec, ts);  
            output_vec.clear();

            //creo il vettore dei valori di input per i neuroni ricevuti dal neurone dello stesso layer che ha fatto la spike, tramite prodotto vec x mat
            // Creation of the input vector with values recived by neurons in the same layers. We use a dot product.
            let intra_layer_input_val = current_spike_vec.dot(&layer.intra_weights);

            // per ogni neurone, attivo la funzione handle_spike coi suoi parametri e le sue variabili, 
            // prese dai vettori inizializzati precedentemente
            // raccolgo l'output nel vettore
            // Gestisce gli input dai neuroni dello stesso layer

            // For each neuron in the layer, we use the `handle_spike` function given the neuron parameters and variables and 
            // the computed input considering the intra-layer links. We can obtain a spike (`1`) or not (`0`) 
            for (i, neuron) in layer.neurons.iter().enumerate(){
                
                M::handle_spike(neuron, 
                    &mut neuron_vars[i].vars, 
                    intra_layer_input_val[[0,i]], 
                    ts);
            }
        }

        out_spikes
        


    }

    //TODO CERCARE DI UNIRE QUESTA FUNZIONE ALLA INFER_SPIKE

    /*fn apply_spike_to_input_layer_neuron(
                                neuron_id: usize, 
                                ts: u128, 
                                network: &NN<M>, 
                                sim_network: &mut SimulatedNN<M>)-> Array2<f64> {

        //get dimension of the input layer
        let n_neurons_layer0 = network.layers[0].neurons.len();

        //input val for neuron_id-th neuron is 1 times the corresponding input_weight
        let weighted_input_val: f64 = network.layers[0].input_weights[(0, neuron_id)];  

        //Obtain the neuron_id-th neuron (parameters and variables) from the input layer 
        let neuron_params = &network.layers[0].neurons[neuron_id];
        let neuron_vars = &mut sim_network.layers[0][neuron_id].vars;

        //faccio handle_spike(spike) e ritiriamo il suo output (una sorta di spike ma per gestione interna)
        let spike_result = M::handle_spike(neuron_params, neuron_vars, weighted_input_val, ts);
        
        //vettore con un solo elemento a 1 in posizione neuro_id-esima
        let mut vec_spike: Vec<f64> = Vec::new();
        
        let arr_spike = single_spike_to_vec(neuron_id, n_neurons_layer0);

        let intra_layer_weights = &network.layers[0].intra_weights;
        
        //Vettore di valori da dare agli altri neuroni dello stesso layer
        let intra_layer_weighted_val = arr_spike.dot(intra_layer_weights);

        //Per ogni altro neurone del layer (Tutti tranne quello che riceve la 
        //spike in ingresso) calcoliamo la nuova tensione
        for n_id in 0..n_neurons_layer0 {
            if n_id != neuron_id{
                let neuron = &network.layers[0].neurons[n_id];
                let sim_neuron = &mut sim_network.layers[0][n_id].vars;
                M::handle_spike(
                        neuron, 
                        sim_neuron,  
                        intra_layer_weighted_val[[n_id,0]], 
                        ts);           
            }
        }
        
        return arr_spike;
    }*/



    
    /*
    pub fn SIMULT_solve(&mut self){

        //[{1, 1}, {2, 3}, {2, 2}, {3,4}]
        let mut t_current = self.input_spikes[0].ts;
        let mut vec_nid = Vec::new();

        for spike in self.input_spikes.iter() {
            
            //se
            if spike.ts != t_current {

                //elabora le spike all'istante precedente
                Self::apply_spike_to_input_layer_neuron(vec_nid, t_current, &mut self.network);
                vec_nid = Vec::new();

                // Aggiorna per la spike al tempo corrente
                vec_nid.push(spike.neuron_id);
                t_current = spike.ts;
            }
            else{
                vec_nid.push(spike.neuron_id);
            }

            //TODO gestire simultaneità
        }

        // Gestione dell'ultima spike..
        Self::apply_spike_to_input_layer_neuron(vec_nid, t_current, &mut self.network)
    }


    fn SIMULT_apply_spike_to_input_layer_neuron(vec_neuron_id: Vec<usize>, ts: u128, network: &mut NN<M>) {

        //[2 ]
        let n_neurons_layer0 = network.layers[0].0.len();
        let mut input_vec : Vec<f64>= Vec::with_capacity(n_neurons_layer0);
        let mut index = 0;

        //costruisce il vettore di spike per il primo layer di input al tempo t_current
        for i in 0..input_vec.len() {
            
            if vec_neuron_id.contains(&i){
                input_vec[i] = 1.;
            }
            else{
                input_vec[i] = 0.;
            }
        }

        let mut weighted_input_val: Vec<f64> = Vec::new();

        for (&n, &w) in input_vec.iter().zip(network.input_weights.iter()) {
            weighted_input_val.push(n*w);  
        }
        
        let intra_layer_weights = network.layers[0].1;
        for ((&n, &w), ind) in input_vec.iter().zip(intra_layer_weights.iter()).enumerate() {
            weighted_input_val[ind] += n*w;  
        }
        
        
        //Per ogni neurone nel vettore vec_id (che hanno le spike simultanee)
        for &neuron_id in vec_neuron_id.iter(){
            //prendo il neurone n_id-esimo dal layer
            let neuron = &mut network.layers[0].0[neuron_id];
            
            //faccio handle_spike(spike) e ritiriamo il suo output (una sorta di spike ma per gestione interna)
            //TODO gestione intralayer
            let results = M::handle_spike(neuron, weighted_input_val[neuron_id]);

        }


        //TODO gestione intralayer
       

        //creo quindi un vettore di output del primo layer

        //moltiplichiamo il vettore di output per la matrice dei pesi (riga-> (vettore di spike)' x matrice -> matrice_pesi)'
        //e otteniamo il vettore di input per il layer successivo
        
    }
    */
}

    /// Create a zero array, but with a single '1' in the neuron_id-th position
    /// 
    /// # Example 
    /// 
    ///  TODO @marcopra (prova)
    fn single_spike_to_vec(neuron_id: usize, dim: usize) -> ArrayBase<OwnedRepr<f64>, Dim<[usize; 2]>> {

        let mut res: Vec<f64> = Vec::new();

        for i in 0..dim {
            if i == neuron_id {res.push(1.);}
            else {res.push(0.)}
        }
        Array2::from_shape_vec([1, dim], res).unwrap()
    }

    /// Create a vec of u128 (val_to_set) starting from a f64 array and a val to use if the f64 is greater than 0 
    /// 
    /// If in the i-th position the val of he input vec is greater than 0, the new vec will have 'val_to_set in that position, otherwise it will have a 0
    fn to_u128_vec<'a, T>(vec: T, val_to_set: u128) -> Vec<u128>
    where T: IntoIterator<Item = &'a f64>
    {
        let mut res: Vec<u128> =  Vec::new();

        for &val in vec {
            if val > 0.0 { res.push(val_to_set)}
            else {res.push(u128::MAX)};
        }   
        res 
    }

#[cfg(test)]
mod tests {
    
    use crate::{lif::{LifNeuronConfig, LeakyIntegrateFire}, NNBuilder, Spike, nn::solver_v1::Solver};

    #[test]
    fn test_init_simulated_nn() {


    }


    #[test]
    fn test_passthrough_nn_using_solver() {

        let config = LifNeuronConfig::new(2.0, 0.5, 2.1, 1.0);
    
        let nn = NNBuilder::<LeakyIntegrateFire, _>::new()
            .layer(
                [
                    From::from(&config),
                    From::from(&config),
                    From::from(&config)
                ],
                [
                    1.0, 1.0, 1.0
                ],
                [
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0]
                ]
            )
            .build();
        
        let spikes = Spike::create_terminal_vec(
            vec![
                Spike::spike_vec_for(0, vec![1, 2, 3, 5, 6, 7]),
                Spike::spike_vec_for(1, vec![2, 6, 7, 9]),
                Spike::spike_vec_for(2, vec![2, 5, 6, 10, 11])
            ]
        );

        let mut solver = Solver::new(spikes, nn);

        assert_eq!(
            solver.solve(),
            vec![
                vec![1, 2, 3, 5, 6, 7],
                vec![2, 6, 7, 9],
                vec![2, 5, 6, 10, 11]
            ]
        );
    }

    #[test]
    fn test_correct_management_of_example_spike(){

    }
}