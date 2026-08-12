use agentx_application::{
    RuntimeCommand, RuntimeCommandType, RuntimeEventEnvelope, StartExecutionCommandPayload,
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use sqlx::{MySql, Row, Transaction};
use uuid::Uuid;

use crate::runtime_commands::RuntimeCommandRepository;

pub(crate) async fn project_evaluation_execution(
    transaction: &mut Transaction<'_, MySql>,
    event: &RuntimeEventEnvelope,
    execution_id: Uuid,
    execution_result: Option<&Value>,
) -> Result<()> {
    if !matches!(
        event.event_type.as_str(),
        "execution.succeeded" | "execution.failed" | "execution.cancelled" | "execution.timed_out"
    ) {
        return Ok(());
    }
    if let Some(case) = sqlx::query("SELECT c.id,c.evaluation_run_id,c.source_case_id,c.status,er.dataset_version_id,er.evaluation_profile_version_id,er.created_by,e.result_json,e.duration_ms,e.cost_micros FROM evaluation_run_cases c JOIN evaluation_runs er ON er.id=c.evaluation_run_id AND er.tenant_id=c.tenant_id JOIN workflow_executions e ON e.id=c.target_execution_id AND e.tenant_id=c.tenant_id WHERE c.tenant_id=? AND c.target_execution_id=? FOR UPDATE")
        .bind(event.tenant_id.as_uuid())
        .bind(execution_id)
        .fetch_optional(&mut **transaction)
        .await?
    {
        project_target_result(transaction, event, execution_id, case, execution_result).await?;
        return Ok(());
    }
    if let Some(rule) = sqlx::query("SELECT rr.id,rr.evaluation_run_case_id,rr.status,e.result_json,e.duration_ms,e.cost_micros FROM evaluation_rule_results rr JOIN workflow_executions e ON e.id=rr.evaluator_execution_id AND e.tenant_id=rr.tenant_id WHERE rr.tenant_id=? AND rr.evaluator_execution_id=? FOR UPDATE")
        .bind(event.tenant_id.as_uuid())
        .bind(execution_id)
        .fetch_optional(&mut **transaction)
        .await?
    {
        project_evaluator_result(transaction, event, rule, execution_result).await?;
    }
    Ok(())
}

async fn project_target_result(
    transaction: &mut Transaction<'_, MySql>,
    event: &RuntimeEventEnvelope,
    execution_id: Uuid,
    case: sqlx::mysql::MySqlRow,
    execution_result: Option<&Value>,
) -> Result<()> {
    let case_id: Uuid = case.try_get("id")?;
    if matches!(
        case.try_get::<String, _>("status")?.as_str(),
        "completed" | "failed" | "cancelled"
    ) {
        return Ok(());
    }
    let run_id: Uuid = case.try_get("evaluation_run_id")?;
    let source_case_id: Uuid = case.try_get("source_case_id")?;
    let duration_ms: Option<u64> = case.try_get("duration_ms")?;
    let cost_micros: u64 = case.try_get("cost_micros")?;
    if event.event_type != "execution.succeeded" {
        let status = if event.event_type == "execution.cancelled" {
            "cancelled"
        } else {
            "failed"
        };
        sqlx::query("UPDATE evaluation_run_cases SET status=?,duration_ms=?,cost_micros=?,error_code=?,error_message=?,completed_at=CURRENT_TIMESTAMP(6) WHERE id=?")
            .bind(status)
            .bind(duration_ms)
            .bind(cost_micros)
            .bind(event.payload.get("code").and_then(Value::as_str).unwrap_or("EVALUATION_TARGET_FAILED"))
            .bind(event.payload.get("message").and_then(Value::as_str).unwrap_or("Target Workflow Execution did not succeed"))
            .bind(case_id)
            .execute(&mut **transaction)
            .await?;
        sqlx::query("INSERT INTO evaluation_case_results(id,tenant_id,evaluation_run_id,source_case_id,execution_id,status,score,detail_json,duration_ms,cost_micros) VALUES(?,?,?,?,?,?,NULL,?,?,?) ON DUPLICATE KEY UPDATE status=VALUES(status),detail_json=VALUES(detail_json),duration_ms=VALUES(duration_ms),cost_micros=VALUES(cost_micros)")
            .bind(Uuid::now_v7())
            .bind(event.tenant_id.as_uuid())
            .bind(run_id)
            .bind(source_case_id)
            .bind(execution_id)
            .bind(if status == "cancelled" { "cancelled" } else { "error" })
            .bind(json!({"runtimeEventId":event.event_id,"eventType":event.event_type}))
            .bind(duration_ms)
            .bind(cost_micros)
            .execute(&mut **transaction)
            .await?;
        finalize_run(transaction, event.tenant_id.as_uuid(), run_id).await?;
        return Ok(());
    }

    let result = execution_result.context("Evaluation target Execution has no result")?;
    let actual =
        workflow_output(result).context("Evaluation target Execution has no End outputs")?;
    let dataset_version_id: Uuid = case.try_get("dataset_version_id")?;
    let expected: Option<Value> = sqlx::query_scalar("SELECT expected_output_json FROM dataset_version_cases WHERE tenant_id=? AND dataset_version_id=? AND source_case_id=?")
        .bind(event.tenant_id.as_uuid())
        .bind(dataset_version_id)
        .bind(source_case_id)
        .fetch_one(&mut **transaction)
        .await?;
    let expected = expected.unwrap_or(Value::Null);
    sqlx::query("UPDATE evaluation_run_cases SET status='scoring',actual_output_json=?,duration_ms=?,cost_micros=? WHERE id=?")
        .bind(&actual)
        .bind(duration_ms)
        .bind(cost_micros)
        .bind(case_id)
        .execute(&mut **transaction)
        .await?;
    let profile_version_id: Uuid = case.try_get("evaluation_profile_version_id")?;
    let created_by: Uuid = case.try_get("created_by")?;
    let rules = sqlx::query("SELECT id,evaluator_type,configuration_json FROM evaluation_profile_rules WHERE tenant_id=? AND profile_version_id=? ORDER BY sort_order,id")
        .bind(event.tenant_id.as_uuid())
        .bind(profile_version_id)
        .fetch_all(&mut **transaction)
        .await?;
    for rule in rules {
        let rule_id: Uuid = rule.try_get("id")?;
        let evaluator_type: String = rule.try_get("evaluator_type")?;
        let configuration: Value = rule.try_get("configuration_json")?;
        let result_id = Uuid::now_v7();
        if matches!(evaluator_type.as_str(), "llm_judge" | "custom_code") {
            let evaluator_version_id = configuration
                .get("evaluatorWorkflowVersionId")
                .and_then(Value::as_str)
                .map(Uuid::parse_str)
                .transpose()?
                .context("Runtime evaluator Workflow Version is missing")?;
            let command = RuntimeCommand::new(
                event.tenant_id,
                RuntimeCommandType::StartExecution,
                "evaluation_rule_result",
                result_id.to_string(),
                format!("evaluation:{run_id}:case:{case_id}:rule:{rule_id}"),
                serde_json::to_value(StartExecutionCommandPayload {
                    workflow_version_id: evaluator_version_id,
                    invocation_id: None,
                    session_id: None,
                    requested_by: Some(created_by),
                    trigger_type: "evaluation_evaluator".into(),
                    input: json!({
                        "actual":actual,
                        "expected":expected,
                        "configuration":configuration,
                        "targetExecutionId":execution_id,
                    }),
                    runtime_settings: json!({
                        "evaluationRunId":run_id,
                        "evaluationCaseId":case_id,
                        "evaluationRuleId":rule_id,
                    }),
                })?,
            );
            RuntimeCommandRepository::enqueue_in_transaction(transaction, &command).await?;
            sqlx::query("INSERT INTO evaluation_rule_results(id,tenant_id,evaluation_run_case_id,profile_rule_id,evaluator_command_id,status,detail_json) VALUES(?,?,?,?,?,'queued',JSON_OBJECT())")
                .bind(result_id)
                .bind(event.tenant_id.as_uuid())
                .bind(case_id)
                .bind(rule_id)
                .bind(command.id)
                .execute(&mut **transaction)
                .await?;
        } else {
            let evaluated = evaluate_rule(&evaluator_type, &configuration, &actual, &expected);
            let (status, passed, score, detail) = match evaluated {
                Ok(passed) => (
                    if passed { "passed" } else { "failed" },
                    Some(passed),
                    Some(if passed { 1.0 } else { 0.0 }),
                    json!({"evaluatorType":evaluator_type}),
                ),
                Err(message) => (
                    "error",
                    None,
                    None,
                    json!({"evaluatorType":evaluator_type,"error":message}),
                ),
            };
            sqlx::query("INSERT INTO evaluation_rule_results(id,tenant_id,evaluation_run_case_id,profile_rule_id,status,passed,score,detail_json,completed_at) VALUES(?,?,?,?,?,?,?,?,CURRENT_TIMESTAMP(6))")
                .bind(result_id)
                .bind(event.tenant_id.as_uuid())
                .bind(case_id)
                .bind(rule_id)
                .bind(status)
                .bind(passed)
                .bind(score)
                .bind(detail)
                .execute(&mut **transaction)
                .await?;
        }
    }
    finalize_case(transaction, event.tenant_id.as_uuid(), case_id).await?;
    Ok(())
}

async fn project_evaluator_result(
    transaction: &mut Transaction<'_, MySql>,
    event: &RuntimeEventEnvelope,
    rule: sqlx::mysql::MySqlRow,
    execution_result: Option<&Value>,
) -> Result<()> {
    if matches!(
        rule.try_get::<String, _>("status")?.as_str(),
        "passed" | "failed" | "error" | "cancelled"
    ) {
        return Ok(());
    }
    let result_id: Uuid = rule.try_get("id")?;
    let case_id: Uuid = rule.try_get("evaluation_run_case_id")?;
    if event.event_type == "execution.succeeded" {
        let evaluated = (|| -> Result<(bool, f64, Value)> {
            let result = execution_result.context("Evaluator Execution has no result")?;
            let output = evaluator_output(result).context(
                "Evaluator Workflow output must be {passed:boolean,score:number,detail:any}",
            )?;
            let passed = output
                .get("passed")
                .and_then(Value::as_bool)
                .context("passed is required")?;
            let score = output
                .get("score")
                .and_then(Value::as_f64)
                .context("score is required")?;
            anyhow::ensure!(
                (0.0..=1.0).contains(&score),
                "Evaluator score must be between 0 and 1"
            );
            Ok((
                passed,
                score,
                output.get("detail").cloned().unwrap_or_else(|| json!({})),
            ))
        })();
        match evaluated {
            Ok((passed, score, detail)) => {
                sqlx::query("UPDATE evaluation_rule_results SET status=?,passed=?,score=?,detail_json=?,duration_ms=?,cost_micros=?,completed_at=CURRENT_TIMESTAMP(6) WHERE id=?")
                    .bind(if passed { "passed" } else { "failed" }).bind(passed).bind(score).bind(detail)
                    .bind(rule.try_get::<Option<u64>, _>("duration_ms")?).bind(rule.try_get::<u64, _>("cost_micros")?).bind(result_id)
                    .execute(&mut **transaction).await?;
            }
            Err(error) => {
                sqlx::query("UPDATE evaluation_rule_results SET status='error',passed=NULL,score=NULL,detail_json=?,duration_ms=?,cost_micros=?,completed_at=CURRENT_TIMESTAMP(6) WHERE id=?")
                    .bind(json!({"code":"EVALUATOR_OUTPUT_INVALID","message":error.to_string()}))
                    .bind(rule.try_get::<Option<u64>, _>("duration_ms")?).bind(rule.try_get::<u64, _>("cost_micros")?).bind(result_id)
                    .execute(&mut **transaction).await?;
            }
        }
    } else {
        sqlx::query("UPDATE evaluation_rule_results SET status=IF(?='execution.cancelled','cancelled','error'),detail_json=?,duration_ms=?,cost_micros=?,completed_at=CURRENT_TIMESTAMP(6) WHERE id=?")
            .bind(&event.event_type)
            .bind(json!({"runtimeEventId":event.event_id,"eventType":event.event_type}))
            .bind(rule.try_get::<Option<u64>, _>("duration_ms")?)
            .bind(rule.try_get::<u64, _>("cost_micros")?)
            .bind(result_id)
            .execute(&mut **transaction)
            .await?;
    }
    finalize_case(transaction, event.tenant_id.as_uuid(), case_id).await?;
    Ok(())
}

async fn finalize_case(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    case_id: Uuid,
) -> Result<()> {
    let pending: u64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS UNSIGNED) FROM evaluation_rule_results WHERE tenant_id=? AND evaluation_run_case_id=? AND status IN ('queued','running')")
        .bind(tenant_id)
        .bind(case_id)
        .fetch_one(&mut **transaction)
        .await?;
    if pending != 0 {
        return Ok(());
    }
    let case = sqlx::query("SELECT c.evaluation_run_id,c.source_case_id,c.target_execution_id,c.actual_output_json,c.duration_ms,c.cost_micros,pv.aggregation,CAST(pv.pass_threshold AS DOUBLE) pass_threshold FROM evaluation_run_cases c JOIN evaluation_runs er ON er.id=c.evaluation_run_id JOIN evaluation_profile_versions pv ON pv.id=er.evaluation_profile_version_id WHERE c.tenant_id=? AND c.id=? FOR UPDATE")
        .bind(tenant_id)
        .bind(case_id)
        .fetch_one(&mut **transaction)
        .await?;
    let scores = sqlx::query("SELECT rr.status,rr.passed,CAST(rr.score AS DOUBLE) score,CAST(pr.weight AS DOUBLE) weight,pr.required FROM evaluation_rule_results rr JOIN evaluation_profile_rules pr ON pr.id=rr.profile_rule_id WHERE rr.tenant_id=? AND rr.evaluation_run_case_id=? ORDER BY pr.sort_order")
        .bind(tenant_id)
        .bind(case_id)
        .fetch_all(&mut **transaction)
        .await?;
    if scores.is_empty() {
        return Ok(());
    }
    let mut weighted_score = 0.0;
    let mut total_weight = 0.0;
    let mut passed_count = 0_u64;
    let mut required_failed = false;
    let mut has_error = false;
    for score in &scores {
        let status: String = score.try_get("status")?;
        let passed = score.try_get::<Option<bool>, _>("passed")?.unwrap_or(false);
        let value = score.try_get::<Option<f64>, _>("score")?.unwrap_or(0.0);
        let weight: f64 = score.try_get("weight")?;
        let required: bool = score.try_get("required")?;
        weighted_score += value * weight;
        total_weight += weight;
        passed_count += u64::from(passed);
        required_failed |= required && !passed;
        has_error |= matches!(status.as_str(), "error" | "cancelled");
    }
    let score = if total_weight == 0.0 {
        0.0
    } else {
        weighted_score / total_weight
    };
    let aggregation: String = case.try_get("aggregation")?;
    let threshold: f64 = case.try_get("pass_threshold")?;
    let passed = !has_error
        && !required_failed
        && match aggregation.as_str() {
            "all" => passed_count == scores.len() as u64,
            "any" => passed_count > 0,
            "weighted" => score >= threshold,
            _ => false,
        };
    let status = if has_error {
        "error"
    } else if passed {
        "passed"
    } else {
        "failed"
    };
    let run_id: Uuid = case.try_get("evaluation_run_id")?;
    sqlx::query("INSERT INTO evaluation_case_results(id,tenant_id,evaluation_run_id,source_case_id,execution_id,status,score,detail_json,duration_ms,cost_micros) VALUES(?,?,?,?,?,?,?,?,?,?) ON DUPLICATE KEY UPDATE status=VALUES(status),score=VALUES(score),detail_json=VALUES(detail_json),duration_ms=VALUES(duration_ms),cost_micros=VALUES(cost_micros)")
        .bind(Uuid::now_v7())
        .bind(tenant_id)
        .bind(run_id)
        .bind(case.try_get::<Uuid, _>("source_case_id")?)
        .bind(case.try_get::<Uuid, _>("target_execution_id")?)
        .bind(status)
        .bind(score)
        .bind(json!({"aggregation":aggregation,"passed":passed,"ruleCount":scores.len(),"actual":case.try_get::<Option<Value>,_>("actual_output_json")?}))
        .bind(case.try_get::<Option<u64>, _>("duration_ms")?)
        .bind(case.try_get::<u64, _>("cost_micros")?)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("UPDATE evaluation_run_cases SET status=IF(?='error','failed','completed'),completed_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND id=?")
        .bind(status)
        .bind(tenant_id)
        .bind(case_id)
        .execute(&mut **transaction)
        .await?;
    finalize_run(transaction, tenant_id, run_id).await?;
    Ok(())
}

pub(crate) async fn finalize_evaluation_case(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    case_id: Uuid,
) -> Result<()> {
    finalize_case(transaction, tenant_id, case_id).await
}

async fn finalize_run(
    transaction: &mut Transaction<'_, MySql>,
    tenant_id: Uuid,
    run_id: Uuid,
) -> Result<()> {
    let remaining: u64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS UNSIGNED) FROM evaluation_run_cases WHERE tenant_id=? AND evaluation_run_id=? AND status IN ('queued','running','scoring')")
        .bind(tenant_id)
        .bind(run_id)
        .fetch_one(&mut **transaction)
        .await?;
    if remaining != 0 {
        sqlx::query("UPDATE evaluation_runs SET status='running' WHERE tenant_id=? AND id=? AND status='queued'")
            .bind(tenant_id)
            .bind(run_id)
            .execute(&mut **transaction)
            .await?;
        return Ok(());
    }
    let metrics = sqlx::query("SELECT CAST(COUNT(*) AS UNSIGNED) total,CAST(COALESCE(SUM(status='passed'),0) AS UNSIGNED) passed,CAST(COALESCE(AVG(score),0) AS DOUBLE) average_score,CAST(COALESCE(SUM(cost_micros),0) AS DOUBLE) total_cost,CAST(COALESCE(AVG(duration_ms),0) AS DOUBLE) average_duration,CAST(COALESCE(SUM(status='error'),0) AS UNSIGNED) errors FROM evaluation_case_results WHERE tenant_id=? AND evaluation_run_id=?")
        .bind(tenant_id)
        .bind(run_id)
        .fetch_one(&mut **transaction)
        .await?;
    let total: u64 = metrics.try_get("total")?;
    let passed: u64 = metrics.try_get("passed")?;
    let values = [
        (
            "pass_rate",
            if total == 0 {
                0.0
            } else {
                passed as f64 / total as f64
            },
        ),
        ("average_score", metrics.try_get::<f64, _>("average_score")?),
        (
            "total_cost_micros",
            metrics.try_get::<f64, _>("total_cost")?,
        ),
        (
            "average_duration_ms",
            metrics.try_get::<f64, _>("average_duration")?,
        ),
    ];
    for (key, value) in values {
        sqlx::query("INSERT INTO evaluation_metrics(tenant_id,evaluation_run_id,metric_key,metric_value,detail_json) VALUES(?,?,?,?,JSON_OBJECT()) ON DUPLICATE KEY UPDATE metric_value=VALUES(metric_value),detail_json=VALUES(detail_json)")
            .bind(tenant_id)
            .bind(run_id)
            .bind(key)
            .bind(value)
            .execute(&mut **transaction)
            .await?;
    }
    let errors: u64 = metrics.try_get("errors")?;
    sqlx::query("UPDATE evaluation_runs SET status=IF(? > 0,'failed','completed'),completed_at=CURRENT_TIMESTAMP(6) WHERE tenant_id=? AND id=? AND status<>'cancelled'")
        .bind(errors)
        .bind(tenant_id)
        .bind(run_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

fn workflow_output(result: &Value) -> Option<Value> {
    result.get("outputs").cloned()
}

fn evaluator_output(result: &Value) -> Option<Value> {
    let output = workflow_output(result)?;
    if output.get("passed").is_some() {
        return Some(output);
    }
    output
        .get("main")
        .and_then(Value::as_array)
        .and_then(|items| items.last())
        .and_then(|item| item.get("json"))
        .cloned()
}

fn evaluate_rule(
    kind: &str,
    configuration: &Value,
    actual: &Value,
    expected: &Value,
) -> Result<bool, String> {
    match kind {
        "exact" => Ok(actual == expected),
        "contains" => Ok(actual
            .as_str()
            .unwrap_or_default()
            .contains(expected.as_str().unwrap_or_default())),
        "regex" => {
            let pattern = configuration
                .get("pattern")
                .and_then(Value::as_str)
                .ok_or_else(|| "pattern is required".to_owned())?;
            regex::Regex::new(pattern)
                .map(|value| value.is_match(actual.as_str().unwrap_or_default()))
                .map_err(|error| error.to_string())
        }
        "json_schema" => {
            let schema = configuration
                .get("schema")
                .ok_or_else(|| "schema is required".to_owned())?;
            let validator = jsonschema::validator_for(schema).map_err(|error| error.to_string())?;
            validator
                .validate(actual)
                .map_err(|error| error.to_string())?;
            Ok(true)
        }
        other => Err(format!("unsupported deterministic evaluator {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{evaluate_rule, evaluator_output};
    use serde_json::{Value, json};

    #[test]
    fn evaluator_output_reads_explicit_end_outputs() {
        let result = json!({"schemaVersion":"4.0","outputs":{"main":[{"json":{"passed":true,"score":0.8,"detail":{}}}]}});
        assert_eq!(evaluator_output(&result).unwrap()["score"], 0.8);
    }

    #[test]
    fn deterministic_rules_are_real() {
        assert!(evaluate_rule("exact", &json!({}), &json!(1), &json!(1)).unwrap());
        assert!(
            evaluate_rule(
                "regex",
                &json!({"pattern":"^ok"}),
                &json!("okay"),
                &Value::Null
            )
            .unwrap()
        );
    }
}
