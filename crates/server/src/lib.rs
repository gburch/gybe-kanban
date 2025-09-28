pub mod activity_feed;
pub mod error;
pub mod mcp;
pub mod middleware;
pub mod routes;
pub mod websocket;

// #[cfg(feature = "cloud")]
// type DeploymentImpl = vibe_kanban_cloud::deployment::CloudDeployment;
// #[cfg(not(feature = "cloud"))]
pub type DeploymentImpl = local_deployment::LocalDeployment;
