use chrono::Utc;
use serde_json::json;

fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    for required in ["--action", "--target", "--backup-id", "--values"] {
        if !arguments.iter().any(|argument| argument == required) {
            eprintln!("missing required argument: {required}");
            std::process::exit(2);
        }
    }
    println!(
        "{}",
        json!({
            "status":"passed",
            "recoveryPointUtc":Utc::now().to_rfc3339(),
            "objectCount":1,
            "contentSha256":"0".repeat(64),
            "schemaVersionObserved":"fixture-v1"
        })
    );
}
