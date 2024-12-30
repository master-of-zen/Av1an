use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Scene {
    pub start_frame: usize,
    // Reminding again that end_frame is *exclusive*
    pub end_frame:   usize,
}
