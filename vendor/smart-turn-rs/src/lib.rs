use anyhow::{Result, anyhow, ensure};
use ndarray::Array3;
use ort::{
    init,
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};
use std::path::Path;
pub mod features;

pub struct PredictionResult {
    pub prediction: u8,
    pub probability: f32,
}

pub struct SmartTurnPredictor {
    pub session: Session,
}

impl SmartTurnPredictor {
    pub fn new(model_path: &Path) -> Result<Self> {
        init().with_name("smart-turn-v3").commit();

        let session = Session::builder().map_err(|error| anyhow!(error.to_string()))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|error| anyhow!(error.to_string()))?
            .with_inter_threads(1)
            .map_err(|error| anyhow!(error.to_string()))?
            .with_intra_threads(1)
            .map_err(|error| anyhow!(error.to_string()))?
            .commit_from_file(model_path)
            .map_err(|error| anyhow!(
                "Failed to load ONNX model from {}: {}",
                model_path.display(),
                error
            ))?;

        Ok(Self { session })
    }

    pub fn predict(&mut self, input_features: Array3<f32>) -> Result<PredictionResult> {
        let dims = input_features.dim();
        let shape = [dims.0, dims.1, dims.2];
        let (raw, offset) = input_features.into_raw_vec_and_offset();
        let start = offset.unwrap_or(0);
        ensure!(start == 0, "SmartTurn expects contiguous feature buffers");
        let tensor = Tensor::from_array((shape, raw)).map_err(|error| anyhow!(error.to_string()))?;

        let outputs = self
            .session
            .run(ort::inputs![tensor])
            .map_err(|error| anyhow!(error.to_string()))?;

        let out_val = &outputs[0];
        let (_shape, data) = out_val
            .try_extract_tensor::<f32>()
            .map_err(|error| anyhow!(error.to_string()))?;
        let probability = data[0];
        let prediction = if probability > 0.5 { 1 } else { 0 };
        Ok(PredictionResult {
            prediction,
            probability,
        })
    }
}
