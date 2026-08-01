use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse<'a> {
    pub service: &'a str,
    pub status: &'a str,
    pub version: &'a str,
}
