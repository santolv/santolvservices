use serde_json::{Value, json};

pub fn document() -> Value {
    let mut paths = serde_json::Map::new();
    let definitions = [
        ("/health/live", "get", "Liveness", ""),
        ("/health/ready", "get", "Database and disk readiness", ""),
        (
            "/api/v1/auth/login",
            "post",
            "Login using username/password; cookie for web, bearer for mobile",
            "username,password,mobile?",
        ),
        ("/api/v1/auth/logout", "post", "Revoke current session", ""),
        (
            "/api/v1/auth/password",
            "post",
            "Change own password and revoke all sessions",
            "current_password,new_password",
        ),
        (
            "/api/v1/admin/users",
            "get",
            "Server administrator: list user accounts, never their files",
            "",
        ),
        (
            "/api/v1/admin/users",
            "post",
            "Server administrator: create user with temporary password",
            "username,password",
        ),
        (
            "/api/v1/admin/users/{id}",
            "patch",
            "Server administrator: disable or enable user; files preserved",
            "disabled",
        ),
        (
            "/api/v1/admin/users/{id}/password",
            "post",
            "Server administrator: reset temporary password and revoke sessions",
            "password",
        ),
        (
            "/api/v1/admin/audit",
            "get",
            "Server administrator: management audit cursor (integer after)",
            "",
        ),
        ("/api/v1/me", "get", "Current account", ""),
        (
            "/api/v1/shared",
            "get",
            "Direct shares, cursor by node UUID",
            "",
        ),
        ("/api/v1/spaces", "get", "Member spaces", ""),
        ("/api/v1/spaces", "post", "Create organization", "name"),
        (
            "/api/v1/spaces/{space}/members",
            "get",
            "List organization members",
            "",
        ),
        (
            "/api/v1/spaces/{space}/members",
            "put",
            "Set membership for an existing active user",
            "username,role",
        ),
        (
            "/api/v1/spaces/{space}/members/{user}",
            "delete",
            "Remove member and direct grants; preserve organization files",
            "",
        ),
        (
            "/api/v1/spaces/{space}/nodes",
            "get",
            "List children or directly trashed items",
            "",
        ),
        (
            "/api/v1/spaces/{space}/nodes",
            "post",
            "Create folder",
            "name,parent_id?",
        ),
        (
            "/api/v1/nodes/{id}",
            "get",
            "Metadata and revision ETag",
            "",
        ),
        (
            "/api/v1/nodes/{id}",
            "patch",
            "Rename/move within space; duplicates permitted",
            "name?,parent_id?,move_to_root?",
        ),
        (
            "/api/v1/nodes/{id}/trash",
            "post",
            "Trash a node, hiding its descendants",
            "",
        ),
        (
            "/api/v1/nodes/{id}/restore",
            "post",
            "Restore original parent; restore trashed parent first",
            "",
        ),
        (
            "/api/v1/nodes/{id}/purge",
            "post",
            "Queue irreversible deletion after retention",
            "",
        ),
        ("/api/v1/nodes/{id}/grants", "get", "List direct grants", ""),
        (
            "/api/v1/nodes/{id}/grants",
            "put",
            "Set user or space grant, role 1/2/3",
            "user_id?,subject_space_id?,role",
        ),
        (
            "/api/v1/nodes/{id}/grants/revoke",
            "post",
            "Revoke one direct grant",
            "user_id?,subject_space_id?",
        ),
        (
            "/api/v1/nodes/{id}/versions",
            "get",
            "List immutable versions by UUID cursor",
            "",
        ),
        (
            "/api/v1/nodes/{id}/versions/{version}/restore",
            "post",
            "Create version referencing previous immutable content",
            "",
        ),
        (
            "/api/v1/nodes/{id}/content",
            "get",
            "Stream current or selected version; single byte range supported",
            "",
        ),
        (
            "/api/v1/uploads",
            "post",
            "Reserve quota and start resumable upload",
            "space_id,parent_id?,node_id?,name,size,sha256",
        ),
        (
            "/api/v1/uploads/{id}",
            "get",
            "Get confirmed byte offset and state",
            "",
        ),
        (
            "/api/v1/uploads/{id}",
            "delete",
            "Cancel pending upload and release reservation",
            "",
        ),
        (
            "/api/v1/uploads/{id}/content",
            "patch",
            "Append one verified chunk at confirmed offset",
            "",
        ),
        (
            "/api/v1/uploads/{id}/complete",
            "post",
            "Verify, durably publish, commit ready; safe to retry",
            "",
        ),
    ];
    for (path, method, summary, fields) in definitions {
        let public = path.starts_with("/health/") || path == "/api/v1/auth/login";
        let mut op = json!({"summary":summary,"responses":{
            "200":{"description":"Success; see README for response shapes"},
            "201":{"description":"User account created; initial password must be changed"},
            "204":{"description":"Operation completed without response body"},
            "202":{"description":"Durable background operation accepted; poll resource status"},
            "400":{"description":"Invalid input"},"401":{"description":"Authentication required"},
            "403":{"description":"CSRF or origin rejected"},"404":{"description":"Absent or inaccessible"},
            "409":{"description":"Conflict, quota exceeded, or wrong upload offset"},
            "412":{"description":"If-Match revision no longer current"},
            "428":{"description":"If-Match required"},"429":{"description":"Rate or concurrency limit"},
            "503":{"description":"Dependency unavailable"},"507":{"description":"Insufficient physical storage"}
        },"security":if public{json!([])}else{json!([{"cookieAuth":[]},{"bearerAuth":[]}])},"parameters":[]});
        let p = op["parameters"].as_array_mut().unwrap();
        for part in path.split('/') {
            if let Some(key) = part.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                p.push(json!({"name":key,"in":"path","required":true,"schema":{"type":"string","format":"uuid"}}));
            }
        }
        if !public && method != "get" {
            p.push(json!({"name":"X-CSRF-Token","in":"header","description":"Required when authenticating with cookie","schema":{"type":"string"}}));
        }
        let idem = path == "/api/v1/uploads"
            || (path == "/api/v1/spaces" && method == "post")
            || (path.ends_with("/nodes") && method == "post")
            || (path.starts_with("/api/v1/nodes/")
                && matches!(method, "patch" | "post")
                && !path.ends_with("purge")
                && !path.contains("grants"));
        if idem {
            p.push(json!({"name":"Idempotency-Key","in":"header","required":true,"description":"1–128 bytes; retained 7 days; same payload replays original result","schema":{"type":"string","maxLength":128}}));
        }
        if path.starts_with("/api/v1/nodes/")
            && matches!(method, "patch" | "post")
            && !path.contains("grants")
        {
            p.push(json!({"name":"If-Match","in":"header","required":true,"description":"Quoted node revision, e.g. \"1\"","schema":{"type":"string"}}));
        }
        if path == "/api/v1/uploads" {
            p.push(json!({"name":"If-Match","in":"header","description":"Required for node_id (new file version)","schema":{"type":"string"}}));
        }
        if method == "get"
            && (path.ends_with("/nodes")
                || path.ends_with("/shared")
                || path.ends_with("/versions")
                || path.ends_with("/members")
                || path == "/api/v1/spaces"
                || path.ends_with("/grants")
                || path == "/api/v1/admin/users"
                || path == "/api/v1/admin/audit")
        {
            p.push(json!({"name":"after","in":"query","schema":if path=="/api/v1/admin/audit"{json!({"type":"integer","format":"int64"})}else{json!({"type":"string","format":"uuid"})}}));
        }
        if method == "get" && path.ends_with("/nodes") {
            for (name, schema) in [
                ("parent", json!({"type":"string","format":"uuid"})),
                (
                    "limit",
                    json!({"type":"integer","minimum":1,"maximum":200,"default":100}),
                ),
                ("trash", json!({"type":"boolean","default":false})),
            ] {
                p.push(json!({"name":name,"in":"query","schema":schema}));
            }
        }
        if method == "get" && path.ends_with("/content") {
            for (name, location) in [
                ("Range", "header"),
                ("If-Range", "header"),
                ("version", "query"),
            ] {
                p.push(json!({"name":name,"in":location,"schema":{"type":"string"}}));
            }
        }
        if method == "patch" && path.ends_with("/content") {
            for name in ["Upload-Offset", "Content-Length", "X-Chunk-SHA256"] {
                p.push(
                    json!({"name":name,"in":"header","required":true,"schema":{"type":"string"}}),
                );
            }
            op["requestBody"] = json!({"required":true,"content":{"application/octet-stream":{"schema":{"type":"string","format":"binary"}}}});
        } else if !fields.is_empty() {
            let mut props = serde_json::Map::new();
            let mut required = Vec::new();
            for field in fields.split(',') {
                let optional = field.ends_with('?');
                let f = field.trim_end_matches('?');
                let schema = if f == "size" {
                    json!({"type":"integer","format":"int64","minimum":0})
                } else if f == "role" {
                    json!({"type":"integer","enum":[1,2,3]})
                } else if f == "mobile" || f == "move_to_root" || f == "disabled" {
                    json!({"type":"boolean","default":false})
                } else if f.ends_with("_id") {
                    json!({"type":"string","format":"uuid","nullable":optional})
                } else {
                    json!({"type":"string"})
                };
                props.insert(f.into(), schema);
                if !optional {
                    required.push(f);
                }
            }
            op["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":{"type":"object","properties":props,"required":required}}}});
        }
        paths.entry(path.to_owned()).or_insert_with(|| json!({}))[method] = op;
    }
    json!({"openapi":"3.0.3","info":{"title":"SantolvDrive","version":"0.1.0","description":"Self-hosted storage. Cookies require CSRF for writes. Role 1 reader, 2 editor, 3 manager. Members see all space contents; external grants are additive and inherited. See README for consistency and deployment guarantees."},"paths":paths,"components":{"securitySchemes":{"cookieAuth":{"type":"apiKey","in":"cookie","name":"drive_session"},"bearerAuth":{"type":"http","scheme":"bearer"}}}})
}
#[cfg(test)]
mod tests {
    #[test]
    fn valid_contract() {
        let d = super::document();
        assert_eq!(d["openapi"], "3.0.3");
        assert!(d["paths"].as_object().unwrap().len() > 25);
        assert!(d["paths"]["/api/v1/uploads"]["post"]["requestBody"].is_object());
    }
}
