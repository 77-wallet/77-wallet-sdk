// #[derive(Debug, serde::Deserialize, Clone)]
// pub struct SaveStrategyReq {
//     pub uid: String,
//     pub threshold: f64,
//     pub normal_index: i32,
//     pub risk_index: i32,
//     pub normal_address: Vec<CustomStrategy>,
//     pub risk_address: Vec<CustomStrategy>,
// }

// #[derive(Debug, serde::Deserialize, Clone)]
// pub struct CustomStrategy {
//     chain_code: String,
//     address: String,
// }

// impl SaveStrategyReq {
//     pub fn new(
//         uid: &str,
//         threshold: f64,
//         normal_index: i32,
//         risk_index: i32,
//         normal_address: Vec<CustomStrategy>,
//         risk_address: Vec<CustomStrategy>,
//     ) -> Self {
//         Self {
//             uid: uid.to_string(),
//             threshold,
//             normal_index,
//             risk_index,
//             normal_address,
//             risk_address,
//         }
//     }
// }
