#[cfg(feature = "dgen")]
use riverbase_core::base::RiverbaseResult;
#[cfg(feature = "dgen")]
use river_duckweed::pipeline::{PipelineRegistry, RunResult};
#[cfg(feature = "dgen")]
use serde_json::{json, Value};

#[cfg(feature = "dgen")]
use crate::spec::Document;

#[cfg(feature = "dgen")]
/// Pass `document.to_json()` as the jinja `context` input to a dgen pipeline.
pub async fn render_document(
    doc: &Document,
    pipeline_name: &str,
    step: Option<&str>,
) -> RiverbaseResult<RunResult> {
    let pipeline = PipelineRegistry::global()
        .get(pipeline_name)
        .ok_or_else(|| crate::errors::FRM_080.with_data(pipeline_name.to_string()))?;
    let inputs: Value = json!({ "context": doc.to_json() });
    pipeline.run_async(step, inputs).await
}
