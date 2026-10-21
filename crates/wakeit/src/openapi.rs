use serde_json::{Value, json};
fn schema<T: schemars::JsonSchema>(name: &str) -> Value {
    let mut value = serde_json::to_value(schemars::schema_for!(T)).expect("schema serialization");
    fn rewrite(value: &mut Value, name: &str) {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(reference)) = map.get_mut("$ref")
                    && reference.starts_with("#/$defs/")
                {
                    *reference = reference.replacen(
                        "#/$defs/",
                        &format!("#/components/schemas/{name}/$defs/"),
                        1,
                    );
                }
                for value in map.values_mut() {
                    rewrite(value, name);
                }
            }
            Value::Array(values) => {
                for value in values {
                    rewrite(value, name);
                }
            }
            _ => {}
        }
    }
    rewrite(&mut value, name);
    value
}
pub fn document() -> Value {
    let mut schemas = serde_json::Map::new();
    macro_rules! add {
        ($name:literal, $ty:ty) => {
            schemas.insert($name.into(), schema::<$ty>($name));
        };
    }
    add!("RetryNode", crate::api::RetryNode);

    add!("FilePreviewRequest", crate::file_api::Preview);
    add!("DagValidationIssue", crate::dag::ValidationIssue);
    add!("Task", crate::config::TaskConfig);
    add!("EditorLayout", crate::editor_layout::EditorLayout);
    schemas.insert("TaskRecord".into(), json!({
        "type":"object", "required":["id","config","revision"],
        "properties":{
            "id":{"type":"string"}, "config":{"$ref":"#/components/schemas/Task"},
            "revision":{"type":"integer"},
            "editor_layout":{"anyOf":[{"$ref":"#/components/schemas/EditorLayout"},{"type":"null"}],"description":"Shared DAG canvas coordinates, outside TaskConfig and execution snapshots. Omitted or null on PUT preserves positions of remaining nodes."}
        }
    }));
    add!("Agent", crate::config::AgentConfig);
    add!("Schedule", crate::config::Schedule);
    add!("Login", crate::api::Login);
    add!("Changelog", crate::changelog::Changelog);
    add!("DeviceIntent", crate::device_auth::Intent);
    add!("DeviceToken", crate::device_auth::TokenRequest);
    add!("RunnerConnect", crate::device_auth::Connect);
    add!("CreateUser", crate::api::CreateUser);
    add!("Password", crate::api::PasswordBody);
    add!("ArchiveTask", crate::api::ArchiveBody);
    add!("MemberUpdate", crate::api::MemberUpdate);
    add!("Role", crate::api::RoleBody);
    add!("AcceptInvite", crate::api::Accept);
    add!("Token", crate::api::TokenBody);
    add!("ApplyTask", crate::api::Apply);
    add!("Resolution", crate::api::Resolution);
    add!("Enroll", crate::api::Enroll);
    add!("Hello", crate::api::Hello);
    add!("Heartbeat", crate::api::Heartbeat);
    add!("Claim", crate::api::Claim);
    add!("ModelsQuery", crate::api::ModelsQuery);
    add!("ModelCatalog", crate::protocol::ModelCatalog);
    add!("StreamRecord", crate::trace::StreamRecord);
    add!("AgentInfo", crate::protocol::AgentInfo);
    add!("SessionInfo", crate::sessions::SessionInfo);
    add!("CreateTerminal", crate::terminal_api::Create);
    add!("RunSession", crate::sessions::RunSession);
    schemas.insert("SessionInspect".into(), json!({
        "type":"object", "additionalProperties":false, "required":["cwd","id"],
        "properties":{
            "cwd":{"type":"string","minLength":1,"description":"Native Runner path within its allowed roots."},
            "id":{"type":"string","minLength":1,"maxLength":256,"description":"Explicit Agent-native session ID; paths and implicit last-session aliases are not accepted."}
        }
    }));
    schemas.insert("SessionListQuery".into(), json!({
        "type":"object", "additionalProperties":false, "required":["cwd"],
        "properties":{"cwd":{"type":"string","minLength":1},"cursor":{"type":["string","null"]},"q":{"type":["string","null"]}}
    }));
    schemas.insert("SessionCatalog".into(), json!({
        "type":"object", "required":["version","sessions"],
        "properties":{"version":{"const":1},"next_cursor":{"type":["string","null"]},"sessions":{"type":"array","items":{"type":"object","required":["id","cwd"],"properties":{"id":{"type":"string"},"title":{"type":["string","null"],"description":"Original native Agent title; null for unnamed sessions. No conversation preview or synthesized title."},"cwd":{"type":"string"},"updated_at":{"type":["string","null"],"format":"date-time"}}}}}
    }));
    let mut node = schema::<crate::dag::Node>("DagRunNode");
    node["additionalProperties"] = json!(true);
    for (key, value) in [
        ("state", json!({"type":"string"})),
        ("reason", json!({"type":["string","null"]})),
        (
            "execution_id",
            json!({"type":["string","null"],"format":"uuid"}),
        ),
        ("continuation_available", json!({"type":"boolean"})),
        ("attempt_count", json!({"type":"integer"})),
        (
            "retry_at",
            json!({"type":["string","null"],"format":"date-time"}),
        ),
        (
            "started_at",
            json!({"type":["string","null"],"format":"date-time"}),
        ),
        (
            "finished_at",
            json!({"type":["string","null"],"format":"date-time"}),
        ),
    ] {
        node["properties"][key] = value;
    }
    schemas.insert("DagRunNode".into(), node);
    schemas.insert("RunEvent".into(),json!({"type":"object","required":["id","run_id","seq","kind","data"],"properties":{
      "id":{"type":"integer"},"run_id":{"type":"string","format":"uuid"},"seq":{"type":"integer"},"kind":{"type":"string"},"data":{},
      "node_id":{"type":"string"},"execution_id":{"type":"string","format":"uuid"},"original_seq":{"type":"integer"}
    }}));
    schemas.insert("MomentOutput".into(),json!({"type":"object","required":["node_id","name","state","preview","format","has_more","truncated"],"properties":{
      "node_id":{"type":"string"},"name":{"type":"string"},"state":{"type":"string"},"reason":{"type":["string","null"]},"preview":{"type":["string","null"],"maxLength":2048},"format":{"enum":["markdown","json"]},"has_more":{"type":"boolean"},"truncated":{"type":"boolean"}
    }}));
    schemas.insert("Run".into(), json!({
        "type":"object", "required":["id","task_id","config","state","session"],
        "properties":{
            "id":{"type":"string","format":"uuid"},"task_id":{"type":"string"},
            "config":{"$ref":"#/components/schemas/Task"},"state":{"type":"string","enum":["queued","starting","running","unknown","succeeded","warning","failed","timed_out","cancelled","skipped","interrupted"]},"nodes":{"type":"array","items":{"$ref":"#/components/schemas/DagRunNode"}},"parent_run_id":{"type":["string","null"],"format":"uuid"},"node_id":{"type":["string","null"]},
            "task_archived":{"type":"boolean"},
            "session":{"anyOf":[{"$ref":"#/components/schemas/RunSession"},{"type":"null"}],"description":"Actual session captured by this execution. Null for historical or unsupported runs; a configured binding alone does not establish continuation readiness."},
            "result":{},"reason":{"type":["string","null"]},
            "started_at":{"type":["string","null"],"format":"date-time"},
            "finished_at":{"type":["string","null"],"format":"date-time"},
            "attempt_origin":{"enum":["initial","automatic","manual","continuation"]},
            "recovery_generation":{"type":"integer","minimum":0},
            "window_started_at":{"type":["string","null"],"format":"date-time"}
        }
    }));
    schemas.insert("Settings".into(), json!({"type":"object","required":["timezone","log_retention_days","run_retention_days"],"properties":{"timezone":{"type":"string"},"log_retention_days":{"type":"integer","minimum":1,"maximum":3650},"run_retention_days":{"type":"integer","minimum":1,"maximum":3650}}}));
    schemas.insert(
        "Error".into(),
        json!({"type":"object","required":["error"],"properties":{"error":{"type":"string"}}}),
    );
    let mut paths = serde_json::Map::new();
    for (path, methods) in [
        ("/auth/github/start", vec!["get"]),
        ("/auth/github/callback", vec!["get"]),
        ("/meta", vec!["get"]),
        ("/changelog", vec!["get"]),
        ("/runners/{id}/directories", vec!["get"]),
        ("/runners/{id}/agents/{agent}/models/query", vec!["post"]),
        (
            "/runners/{id}/agents/{agent}/sessions/inspect",
            vec!["post"],
        ),
        ("/runners/{id}/agents/{agent}/sessions/query", vec!["post"]),
        ("/oauth/device/code", vec!["post"]),
        ("/oauth/token", vec!["post"]),
        ("/device-authorizations/{user_code}", vec!["get"]),
        ("/device-authorizations/{user_code}/approve", vec!["post"]),
        ("/device-authorizations/{user_code}/deny", vec!["post"]),
        ("/runners/connect", vec!["post"]),
        ("/session", vec!["get", "post", "delete"]),
        ("/tasks", vec!["get"]),
        ("/tasks/{id}", vec!["get", "put"]),
        ("/tasks/validate", vec!["post"]),
        ("/tasks/{id}/runs", vec!["post"]),
        ("/tasks/{id}/archive", vec!["post"]),
        ("/tasks/{id}/restore", vec!["post"]),
        ("/moments/days", vec!["get"]),
        ("/moments", vec!["get"]),
        ("/runs", vec!["get"]),
        ("/runs/{id}", vec!["get"]),
        ("/runs/{id}/files/preview", vec!["post"]),
        ("/runs/{id}/terminals", vec!["post"]),
        ("/terminals/{id}", vec!["get"]),
        ("/terminals/{id}/attach", vec!["post"]),
        ("/terminals/{id}/stream", vec!["get"]),
        ("/terminals/{id}/close", vec!["post"]),
        ("/runs/{id}/events", vec!["get"]),
        ("/runs/{id}/nodes/{node}", vec!["get"]),
        ("/runs/{id}/nodes/{node}/events", vec!["get"]),
        ("/runs/{id}/nodes/{node}/resolve", vec!["post"]),
        ("/runs/{id}/nodes/{node}/retry", vec!["post"]),
        ("/runs/{id}/stream", vec!["get"]),
        ("/runs/{id}/cancel", vec!["post"]),
        ("/runs/{id}/retry", vec!["post"]),
        ("/runs/{id}/resolve", vec!["post"]),
        ("/runners", vec!["get"]),
        ("/runners/enrollment", vec!["post"]),
        ("/runners/{id}/revoke", vec!["post"]),
        ("/agents", vec!["get"]),
        ("/agents/templates/{id}", vec!["get"]),
        ("/users", vec!["get", "post"]),
        ("/users/{id}", vec!["put"]),
        ("/users/{id}/password", vec!["post"]),
        ("/invites", vec!["post"]),
        ("/invites/accept", vec!["post"]),
        ("/tokens", vec!["get", "post"]),
        ("/tokens/{id}", vec!["delete"]),
        ("/settings", vec!["get", "put"]),
        ("/timezones", vec!["get"]),
        ("/schedule/preview", vec!["post"]),
        ("/audit", vec!["get"]),
        ("/device/enroll", vec!["post"]),
        ("/device/hello", vec!["post"]),
        ("/device/heartbeat", vec!["post"]),
        ("/device/claim", vec!["post"]),
        ("/schema/{kind}", vec!["get"]),
        ("/openapi.json", vec!["get"]),
    ] {
        let mut methods_map = serde_json::Map::new();
        for method in methods {
            let mut op = json!({"operationId":format!("{}_{}",method,path.replace('/',"_").replace(['{','}'],"")),"responses":{"200":{"description":"Success"},"400":{"description":"Invalid input"},"401":{"description":"Authentication required"},"403":{"description":"Insufficient permissions"},"409":{"description":"Conflict"}},"security":[{"bearerAuth":[]},{"sessionCookie":[]}]});
            if path.contains("{id}") {
                op["parameters"] =
                    json!([{"name":"id","in":"path","required":true,"schema":{"type":"string"}}]);
            }
            let body = match (path, method) {
                ("/runs/{id}/nodes/{node}/retry", "post") => Some("RetryNode"),

                ("/runs/{id}/files/preview", "post") => Some("FilePreviewRequest"),
                ("/runs/{id}/terminals", "post") => Some("CreateTerminal"),
                ("/users", "post") => Some("CreateUser"),
                ("/users/{id}/password", "post") => Some("Password"),
                ("/tasks/{id}/archive" | "/tasks/{id}/restore", "post") => Some("ArchiveTask"),
                ("/session", "post") => Some("Login"),
                ("/oauth/device/code", "post") => Some("DeviceIntent"),
                ("/oauth/token", "post") => Some("DeviceToken"),
                ("/runners/connect", "post") => Some("RunnerConnect"),
                ("/runners/{id}/agents/{agent}/models/query", "post") => Some("ModelsQuery"),
                ("/runners/{id}/agents/{agent}/sessions/inspect", "post") => Some("SessionInspect"),
                ("/runners/{id}/agents/{agent}/sessions/query", "post") => Some("SessionListQuery"),
                ("/tasks/{id}", "put") | ("/tasks/validate", "post") => Some("ApplyTask"),
                ("/runs/{id}/resolve" | "/runs/{id}/nodes/{node}/resolve", "post") => {
                    Some("Resolution")
                }
                ("/users/{id}", "put") => Some("MemberUpdate"),
                ("/invites", "post") => Some("Role"),
                ("/invites/accept", "post") => Some("AcceptInvite"),
                ("/tokens", "post") => Some("Token"),
                ("/settings", "put") => Some("Settings"),
                ("/schedule/preview", "post") => Some("Schedule"),
                ("/device/enroll", "post") => Some("Enroll"),
                ("/device/hello", "post") => Some("Hello"),
                ("/device/heartbeat", "post") => Some("Heartbeat"),
                ("/device/claim", "post") => Some("Claim"),
                _ => None,
            };
            if path == "/tasks/{id}" && matches!(method, "get" | "put") {
                op["responses"]["200"]["content"] = json!({"application/json":{"schema":{"$ref":"#/components/schemas/TaskRecord"}}});
            }
            if let Some(name) = body {
                op["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{name}")}}}});
            }
            if path == "/terminals/{id}/stream" {
                op["description"] = json!(
                    "Same-origin WebSocket. Send {ticket,cols,rows} in the first text frame within 5 seconds. Ticket is single-use and expires after 30 seconds; never place it in the URL. Client frames: heartbeat, input {seq,data:base64}, resize {cols,rows}, snapshot. Server frames: snapshot, output (base64), state, ack, error. Input is never replayed or persisted. One writer; 5-second heartbeat, 30-second disconnect lease."
                );
                op["responses"]["101"] = json!({"description":"WebSocket upgraded; first-frame authorization is still required."});
            }
            if path == "/terminals/{id}/attach" {
                op["description"] = json!(
                    "Returns {ticket,expires_in:30}. Bound to the same account credential, workspace and terminal. Does not start a process."
                );
            }
            if path == "/runs/{id}/terminals" {
                op["description"] = json!(
                    "Operator only. Creates lifecycle metadata; starts only after authenticated WebSocket attachment with dimensions. Optional node_id selects the actual DAG execution. request_id is idempotent. A disconnected terminal may be reattached within 30 seconds; competing writers return 409."
                );
            }
            if path.starts_with("/terminals/") && path != "/terminals/{id}/stream" {
                op["responses"]["200"]["description"] = json!(
                    "Terminal lifecycle metadata (id,state,generation,reason,exit_code), or a short-lived attachment ticket. No terminal input or screen content is persisted."
                );
            }
            if path.contains("{user_code}") {
                op["parameters"] = json!([{"name":"user_code","in":"path","required":true,"schema":{"type":"string","minLength":8,"maxLength":8}}]);
                op["security"] = json!([{"sessionCookie":[]}]);
            }
            if matches!(
                path,
                "/meta" | "/changelog" | "/oauth/device/code" | "/oauth/token"
            ) {
                op["security"] = json!([]);
            }
            if path.starts_with("/device/") {
                op["security"] = json!([{"deviceToken":[]}]);
            }
            if matches!(path, "/tasks" | "/runs") {
                op["description"] = json!(
                    "With page: {items,total,page,page_size}; without page: an array. Runs without page are limited to 100. q matches literal, case-insensitive text with ordered whitespace-separated terms."
                );
            }
            let parameters = op
                .as_object_mut()
                .unwrap()
                .entry("parameters")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap();
            if path == "/changelog" {
                parameters.push(json!({"name":"lang","in":"query","schema":{"type":"string","enum":["en","zh"],"default":"en"}}));
            }
            if matches!(path, "/moments" | "/moments/days") {
                parameters.push(json!({"name":"limit","in":"query","schema":{"type":"integer","minimum":1,"maximum":100,"default":if path.ends_with("days") {14} else {20}}}));
                if path == "/moments/days" {
                    parameters.push(json!({"name":"before","in":"query","schema":{"type":"string","format":"date"},"description":"Exclusive local-date cursor."}));
                } else {
                    parameters.push(json!({"name":"date","in":"query","required":true,"schema":{"type":"string","format":"date"}}));
                    parameters.push(json!({"name":"cursor","in":"query","schema":{"type":"string"},"description":"Opaque completion time and ID cursor from next_cursor."}));
                    parameters.push(json!({"name":"timezone","in":"query","schema":{"type":"string"},"description":"Expected workspace timezone; mismatch returns 409. Refresh day directory to regroup."}));
                }
            }
            if path == "/tasks" && method == "get" {
                parameters.push(json!({"name":"archived","in":"query","schema":{"type":"boolean","default":false},"description":"Archived tasks only; requires Operator."}));
            }
            if path.contains("{node}") {
                parameters.push(
                    json!({"name":"node","in":"path","required":true,"schema":{"type":"string"}}),
                );
            }
            if path.contains("{agent}") {
                parameters.push(
                    json!({"name":"agent","in":"path","required":true,"schema":{"type":"string"}}),
                );
            }
            if path == "/runners/{id}/directories" {
                for name in ["path", "cursor"] {
                    parameters.push(json!({"name":name,"in":"query","schema":{"type":"string"}}));
                }
            }
            if path == "/runs/{id}/nodes/{node}" {
                parameters.push(json!({"name":"execution","in":"query","schema":{"type":"string","format":"uuid"},"description":"An existing attempt owned by this parent and node. Omit to follow the current execution."}));
            }
            if path == "/schema/{kind}" {
                parameters.push(json!({"name":"kind","in":"path","required":true,"schema":{"type":"string","enum":["task","agent"]}}));
            }
            if matches!(
                path,
                "/tasks/{id}/runs" | "/runs/{id}/retry" | "/runs/{id}/nodes/{node}/retry"
            ) {
                parameters.push(json!({"name":"Idempotency-Key","in":"header","required":true,"schema":{"type":"string","maxLength":200}}));
            }
            if matches!(
                path,
                "/runs/{id}/events" | "/runs/{id}/stream" | "/runs/{id}/nodes/{node}/events"
            ) {
                parameters.push(
                    json!({"name":"after","in":"query","schema":{"type":"integer","default":0}}),
                );
            }
            if path == "/runs/{id}/events" || path == "/runs/{id}/nodes/{node}/events" {
                parameters.push(json!({"name":"kind","in":"query","schema":{"type":"string"},"description":"Filter event kind; trace includes trace_status. Up to 500 rows ordered by ID per page."}));
            }
            if path == "/runs/{id}/stream" {
                parameters.push(
                    json!({"name":"Last-Event-ID","in":"header","schema":{"type":"integer"}}),
                );
            }
            if method == "get" && matches!(path, "/tasks" | "/runs" | "/users" | "/tokens") {
                for (name, default, maximum) in [("page", 1, 1_000_000), ("page_size", 20, 100)] {
                    parameters.push(json!({"name":name,"in":"query","schema":{"type":"integer","minimum":1,"maximum":maximum,"default":default}}));
                }
            }
            if matches!(path, "/tasks" | "/runs") {
                for name in ["q", "runner", "agent"] {
                    parameters.push(json!({"name":name,"in":"query","schema":{"type":"string"}}));
                }
            }
            if path == "/tasks" {
                parameters.push(json!({"name":"enabled","in":"query","schema":{"type":"boolean"}}));
            }
            if path == "/runs" {
                for name in ["task", "state", "before"] {
                    parameters.push(json!({"name":name,"in":"query","schema":{"type":"string"}}));
                }
            }
            for (code, description) in [
                ("400", "Invalid input"),
                ("401", "Authentication required"),
                ("403", "Insufficient permissions"),
                ("404", "Not found"),
                ("409", "Conflict"),
                ("413", "Request too large"),
                ("422", "Invalid JSON shape"),
            ] {
                op["responses"][code] = json!({"description":description,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/Error"}}}});
            }
            op["responses"]["200"]["content"] = if path == "/runs/{id}/stream" {
                json!({"text/event-stream":{"schema":{"type":"string"}}})
            } else if path == "/agents/templates/{id}" {
                json!({"application/yaml":{"schema":{"type":"string"}}})
            } else {
                json!({"application/json":{"schema":{}}})
            };
            if path == "/changelog" {
                op["responses"]["200"] = json!({"description":"Curated Markdown bundled with this Gateway, current version first.","content":{"application/json":{"schema":{"$ref":"#/components/schemas/Changelog"}}}});
            }
            if path == "/moments/days" {
                op["responses"]["200"]["content"] = json!({"application/json":{"schema":{"type":"object","required":["timezone","today","total_runs","days","next_before"],"properties":{"timezone":{"type":"string"},"today":{"type":"string","format":"date"},"total_runs":{"type":"integer"},"days":{"type":"array","items":{"type":"object","required":["date","count"],"properties":{"date":{"type":"string","format":"date"},"count":{"type":"integer"}}}},"next_before":{"type":["string","null"],"format":"date"}}}}});
            }
            if path == "/moments" {
                op["responses"]["200"]["content"] = json!({"application/json":{"schema":{"type":"object","required":["timezone","date","items","next_cursor"],"properties":{"timezone":{"type":"string"},"date":{"type":"string","format":"date"},"next_cursor":{"type":["string","null"]},"items":{"type":"array","items":{"type":"object","required":["id","task_id","name","state","reason","finished_at","runner","agent","logo","preview","format","has_more","truncated"],"properties":{"id":{"type":"string","format":"uuid"},"task_id":{"type":"string"},"name":{"type":"string"},"state":{"type":"string"},"reason":{"type":["string","null"]},"finished_at":{"type":"string","format":"date-time"},"runner":{"type":"string"},"agent":{"type":"string"},"logo":{"type":["string","null"]},"preview":{"type":["string","null"],"maxLength":2048},"format":{"enum":["markdown","json"]},"has_more":{"type":"boolean"},"truncated":{"type":"boolean"}}}}}}}});
                let item = &mut op["responses"]["200"]["content"]["application/json"]["schema"]["properties"]
                    ["items"]["items"];
                item["properties"]["runner"] = json!({"type":["string","null"]});
                item["properties"]["agent"] = json!({"type":["string","null"]});
                item["properties"]["format"] = json!({"enum":["markdown","json","dag"]});
                item["properties"]["node_count"] = json!({"type":"integer"});
                item["properties"]["outputs"] = json!({"type":"array","items":{"$ref":"#/components/schemas/MomentOutput"},"description":"Selected output reports in frozen definition order, sharing a total 2048-Unicode-character preview budget."});
                item["properties"]["continuation_available"] = json!({"type":"boolean","description":"This completed Run has an actual, ready session. Fetch its Run details before displaying continuation commands."});
                item["required"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("continuation_available"));
            }
            if path.ends_with("/events") {
                op["responses"]["200"]["content"] = json!({"application/json":{"schema":{"type":"array","maxItems":500,"items":{"$ref":"#/components/schemas/RunEvent"}}}});
                op["description"] = json!(
                    "Parent DAG events merge child events in durable event-ID order and include node_id, execution_id and original_seq. Child state events have kind node_state. Trace snapshots are keyed by execution_id, node_id and native Trace ID."
                );
            }
            if path == "/tasks/validate" {
                op["responses"]["200"]["content"] = json!({"application/json":{"schema":{"type":"object","required":["errors"],"properties":{"errors":{"type":"array","items":{"$ref":"#/components/schemas/DagValidationIssue"}}}}}});
                op["description"] = json!(
                    "Read-only whole-definition validation for Operator. Does not create or update a task. PUT validates again transactionally."
                );
            }
            if path == "/runs/{id}/nodes/{node}/retry" {
                op["responses"]["200"]["content"] = json!({"application/json":{"schema":{"type":"object","required":["id","parent_run_id","node_id"],"properties":{"id":{"type":"string","format":"uuid"},"parent_run_id":{"type":"string","format":"uuid"},"node_id":{"type":"string"}}}}});
                op["description"] = json!(
                    "Admin/Read-write only. Retries a confirmed failed node in its frozen parent Run, preserving successful nodes and restoring affected descendants under AND. Required nullable execution_id guards stale requests; resume_stopped explicitly recovers graph cancellation/total timeout only after old executions stop. Reuses queued automatic successor. Idempotency-Key returns the same id,parent_run_id,node_id. Node detail exposes retry action, blocking reason and affected descendants."
                );
            }
            if path == "/runs/{id}/nodes/{node}" {
                op["responses"]["200"]["content"] = json!({"application/json":{"schema":{"type":"object","required":["node","execution"],"properties":{"node":{"$ref":"#/components/schemas/DagRunNode"},"execution":{"anyOf":[{"$ref":"#/components/schemas/Run"},{"type":"null"}]},"attempts":{"type":"array","items":{"type":"object","properties":{"id":{"type":"string","format":"uuid"},"state":{"type":"string"},"scheduled_at":{"type":"string","format":"date-time"},"reason":{"type":["string","null"]}}}}}}}});
            }
            if path == "/runs/{id}/nodes/{node}" {
                op["responses"]["200"]["content"]["application/json"]["schema"]["properties"]["retry"] = json!({"type":"object","required":["action","reason","affected"],"properties":{"action":{"type":["string","null"],"enum":["retry","accelerate","resume",null]},"reason":{"type":["string","null"]},"affected":{"type":"array","items":{"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"},"state":{"type":"string"},"action":{"const":"resume"}}}}}});
            }
            if path == "/runs/{id}/nodes/{node}/resolve" {
                op["description"] = json!(
                    "Reconcile an unknown concrete node after checking the actual process. The parent is derived from all nodes; it cannot be independently marked successful."
                );
            }
            if path == "/runs/{id}/files/preview" {
                op["description"] = json!(
                    "Admin/Operator, non-demo only. Reads one explicit link parsed from this exact execution's persisted Markdown result. Relative paths use execution_snapshot.cwd; Runner validates current allowed_roots using a capability-relative file open. No directory input, nested-file authorization, stream or persistent content. Maximum raw content 512 KiB, also bounded by NATS payload. UTF-8 text may be truncated; oversized images are rejected. Response is no-store and nosniff."
                );
                op["responses"]["200"]["content"] = json!({"application/json":{"schema":{"type":"object","required":["path","name","format","content","size","truncated","runner_id"],"properties":{"path":{"type":"string"},"name":{"type":"string"},"format":{"enum":["text","markdown","json","image"]},"mime":{"type":"string"},"content":{"type":"string","description":"UTF-8 text or base64 PNG/JPEG/WebP"},"size":{"type":"integer"},"truncated":{"type":"boolean"},"modified_at":{"type":["string","null"],"format":"date-time"},"runner_id":{"type":"string","format":"uuid"}}}}});
            }
            if path == "/runs/{id}" {
                op["responses"]["200"]["content"] =
                    json!({"application/json":{"schema":{"$ref":"#/components/schemas/Run"}}});
            }
            if path == "/runners/{id}/agents/{agent}/sessions/inspect" {
                op["description"] = json!(
                    "Admin or Operator explicitly verifies an existing session on the selected Runner in cwd. Does not create a Task, Run or session. Runner must be online; this check is advisory when saving and is repeated before execution."
                );
                op["responses"]["200"]["content"] = json!({"application/json":{"schema":{"$ref":"#/components/schemas/SessionInfo"}}});
            }
            if path == "/runners/{id}/agents/{agent}/sessions/query" {
                op["description"] = json!(
                    "Admin or Operator lists existing native sessions on the selected Runner in cwd. Cursor pagination and optional q search are evaluated locally by the Runner over adapter metadata; session_list_supported is independent of session_supported. Returns native title and ID metadata without conversation previews. Does not create sessions or run inference."
                );
                op["responses"]["200"]["content"] = json!({"application/json":{"schema":{"$ref":"#/components/schemas/SessionCatalog"}}});
            }
            if matches!(path, "/schema/{kind}" | "/openapi.json" | "/meta") {
                op["security"] = json!([]);
            }
            if path.starts_with("/auth/github/") {
                op["security"] = json!([]);
                if path.ends_with("/start") {
                    op["parameters"] = json!([{"name":"return_to","in":"query","schema":{"type":"string","maxLength":2048},"description":"A local application path. Absolute, protocol-relative and API paths are rejected."}]);
                    op["responses"]["303"] = json!({"description":"GitHub authorization redirect with S256 PKCE, state and a short-lived HttpOnly SameSite=Lax browser-binding cookie."});
                } else {
                    op["parameters"] = json!([{"name":"state","in":"query","required":true,"schema":{"type":"string"}},{"name":"code","in":"query","schema":{"type":"string"}},{"name":"error","in":"query","schema":{"type":"string"}}]);
                    op["responses"]["200"] = json!({"description":"Same-origin navigation document establishes the Strict Wakeit session before returning to the application. No GitHub access token is returned.","content":{"text/html":{"schema":{"type":"string"}}}});
                    op["responses"]["303"] = json!({"description":"Authorization denied; returns to login without a session."});
                }
            }
            if matches!(path, "/session" | "/invites/accept" | "/device/enroll") && method == "post"
            {
                op["security"] = json!([]);
            }
            methods_map.insert(method.into(), op);
        }
        let global = matches!(
            path,
            "/meta"
                | "/changelog"
                | "/session"
                | "/invites/accept"
                | "/oauth/device/code"
                | "/oauth/token"
                | "/schema/{kind}"
                | "/openapi.json"
        ) || path.starts_with("/device/")
            || path.starts_with("/auth/")
            || path.starts_with("/device-authorizations/");
        let mapped = if path.starts_with("/users") {
            format!("/api/v1/instance{path}")
        } else if global {
            format!("/api/v1{path}")
        } else {
            format!("/api/v1/workspaces/{{workspace}}{path}")
        };
        if !global && !path.starts_with("/users") {
            for op in methods_map.values_mut() {
                if !op["parameters"].is_array() {
                    op["parameters"] = json!([]);
                }
                op["parameters"].as_array_mut().unwrap().push(json!({"name":"workspace","in":"path","required":true,"schema":{"type":"string","format":"uuid"}}));
                op["responses"]["404"] = json!({"description":"Workspace or resource unavailable"});
            }
        }
        paths.insert(mapped, Value::Object(methods_map));
    }
    for (path, methods) in [
        ("/workspaces", vec!["get", "post"]),
        ("/workspaces/{workspace}", vec!["get", "put"]),
        ("/workspaces/{workspace}/leave", vec!["post"]),
        ("/workspaces/{workspace}/members", vec!["get"]),
        (
            "/workspaces/{workspace}/members/{id}",
            vec!["put", "delete"],
        ),
        ("/invites/inspect", vec!["post"]),
        ("/register", vec!["post"]),
    ] {
        let mut operations = serde_json::Map::new();
        for method in methods {
            let mut op = json!({"operationId":format!("{}_{}",method,path.replace('/',"_").replace(['{','}'],"")),"security":[{"sessionCookie":[]}],"responses":{"200":{"description":"Success"},"400":{"description":"Invalid input"},"401":{"description":"Authentication required"},"403":{"description":"Insufficient permissions"},"404":{"description":"Resource unavailable"},"409":{"description":"Conflict"}}});
            let mut params = Vec::new();
            for name in ["workspace", "id"] {
                if path.contains(&format!("{{{name}}}")) {
                    params.push(json!({"name":name,"in":"path","required":true,"schema":{"type":"string","format":"uuid"}}));
                }
            }
            if !params.is_empty() {
                op["parameters"] = json!(params);
            }
            if method == "post" || method == "put" {
                let body = if path == "/workspaces"
                    || (path == "/workspaces/{workspace}" && method == "put")
                {
                    json!({"type":"object","required":["name"],"properties":{"name":{"type":"string","minLength":1,"maxLength":100}}})
                } else if path.ends_with("/members/{id}") {
                    json!({"$ref":"#/components/schemas/MemberUpdate"})
                } else if path == "/invites/inspect" {
                    json!({"type":"object","required":["token"],"properties":{"token":{"type":"string"}}})
                } else if path == "/register" {
                    json!({"$ref":"#/components/schemas/Login"})
                } else {
                    json!({"type":"object"})
                };
                op["requestBody"] =
                    json!({"required":true,"content":{"application/json":{"schema":body}}});
            }
            if matches!(path, "/register" | "/invites/inspect") {
                op["security"] = json!([]);
            }
            operations.insert(method.into(), op);
        }
        paths.insert(format!("/api/v1{path}"), json!(operations));
    }
    json!({"openapi":"3.1.0","info":{"title":"wakeit API","version":env!("CARGO_PKG_VERSION")},"paths":paths,"components":{"securitySchemes":{"bearerAuth":{"type":"http","scheme":"bearer"},"deviceToken":{"type":"http","scheme":"bearer","description":"Runner device token"},"sessionCookie":{"type":"apiKey","in":"cookie","name":"wakeit_session"}},"schemas":schemas}})
}
