use cbm::mcp::{McpServer, SERVER_NAME};
use rmcp::{model::CallToolRequestParams, ServerHandler, ServiceExt};

fn assert_server_handler<T: ServerHandler>() {}

#[test]
fn cbm_uses_official_rmcp_server_handler() {
    assert_server_handler::<McpServer>();
    assert_eq!(SERVER_NAME, "codebase-memory-mcp");
}

#[tokio::test]
async fn official_client_lists_graph_and_rlm_tools() {
    std::env::set_var("CBM_WATCHER", "0");
    let (server_transport, client_transport) = tokio::io::duplex(1024 * 1024);

    let server_task = tokio::spawn(async move {
        McpServer::new()
            .serve(server_transport)
            .await
            .expect("start rmcp server")
            .waiting()
            .await
            .expect("wait rmcp server");
    });

    let client = ().serve(client_transport).await.expect("start rmcp client");
    let tools = client.list_all_tools().await.expect("list tools");

    assert_eq!(tools.len(), 23);
    assert!(tools.iter().any(|tool| tool.name == "index_repository"));
    assert!(tools
        .iter()
        .any(|tool| tool.name == "check_index_coverage"));
    assert!(tools.iter().any(|tool| tool.name == "rlm_workflow"));
    assert!(tools
        .iter()
        .any(|tool| tool.name == "rlm_session_list"));

    let projects = client
        .call_tool(CallToolRequestParams::new("list_projects"))
        .await
        .expect("call list_projects");
    assert_eq!(projects.is_error, Some(false));
    assert!(projects.content[0]
        .raw
        .as_text()
        .is_some_and(|content| content.text.contains("projects")));

    let workflow = client
        .call_tool(CallToolRequestParams::new("rlm_workflow"))
        .await
        .expect("call rlm_workflow");
    assert_eq!(workflow.is_error, Some(false));

    client.cancel().await.expect("cancel client");
    server_task.await.expect("join server task");
}
