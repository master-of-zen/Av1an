use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::models::encoder::Encoder;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(bound = "SequenceData: JsonSchema")]
pub struct Scene<SequenceData> {
    pub start_frame:   usize, // Inclusive
    pub end_frame:     usize, // Exclusive
    pub sub_scenes:    Option<Vec<SubScene>>,
    pub encoder:       Encoder,
    pub sequence_data: SequenceData,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SubScene {
    pub start_frame: usize, // Inclusive
    pub end_frame:   usize, // Exclusive
}
