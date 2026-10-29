use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Thumbnail,
    Original,
}
