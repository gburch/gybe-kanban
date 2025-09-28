use std::path::{Component, Path, PathBuf};

pub(crate) mod activity_feed;

use axum::{
    Extension, Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    middleware::from_fn_with_state,
    response::Json as ResponseJson,
    routing::{get, post, put},
};
use db::models::project::{
    CreateProject, Project, ProjectError, SearchMatchType, SearchResult, UpdateProject,
};
use db::models::project_repository::{
    CreateProjectRepository, ProjectRepository, ProjectRepositoryError, UpdateProjectRepository,
};
use deployment::Deployment;
use ignore::WalkBuilder;
use serde::Deserialize;
use services::services::{
    file_ranker::FileRanker,
    file_search_cache::{CacheError, SearchMode, SearchQuery},
    git::GitBranch,
};
use utils::{path::expand_tilde, response::ApiResponse};
use uuid::Uuid;

use crate::{
    DeploymentImpl, error::ApiError, middleware::load_project_middleware,
    websocket::project_events::project_activity_feed_ws,
};

#[derive(Debug, Deserialize)]
pub struct RepositoryQuery {
    pub repo_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectSearchQuery {
    #[serde(flatten)]
    pub search: SearchQuery,
    pub repo_id: Option<Uuid>,
}

pub async fn get_projects(
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Vec<Project>>>, ApiError> {
    let projects = Project::find_all(&deployment.db().pool).await?;
    Ok(ResponseJson(ApiResponse::success(projects)))
}

pub async fn get_project(
    Extension(project): Extension<Project>,
) -> Result<ResponseJson<ApiResponse<Project>>, ApiError> {
    Ok(ResponseJson(ApiResponse::success(project)))
}

pub async fn get_project_branches(
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
    Query(repo_query): Query<RepositoryQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<GitBranch>>>, ApiError> {
    let pool = &deployment.db().pool;
    let (repo_path, repo_meta): (PathBuf, Option<ProjectRepository>) =
        if let Some(repo_id) = repo_query.repo_id {
            match ProjectRepository::find_by_id(pool, repo_id).await? {
                Some(repo) if repo.project_id == project.id => {
                    let repo_path = repo.git_repo_path.clone();
                    (repo_path, Some(repo))
                }
                Some(_) => {
                    return Ok(ResponseJson(ApiResponse::error(
                        "Repository not found for this project",
                    )));
                }
                None => {
                    return Ok(ResponseJson(ApiResponse::error("Repository not found")));
                }
            }
        } else {
            match ProjectRepository::find_primary(pool, project.id).await? {
                Some(primary) => {
                    let repo_path = primary.git_repo_path.clone();
                    (repo_path, Some(primary))
                }
                None => (project.git_repo_path.clone(), None),
            }
        };

    let mut branches = deployment.git().get_all_branches(&repo_path)?;
    if let Some(repo) = repo_meta.as_ref() {
        for branch in branches.iter_mut() {
            branch.repository_id = Some(repo.id);
            branch.repository_name = Some(repo.name.clone());
        }
    }

    Ok(ResponseJson(ApiResponse::success(branches)))
}
pub async fn get_project_repositories(
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Vec<ProjectRepository>>>, ApiError> {
    let repos = ProjectRepository::list_for_project(&deployment.db().pool, project.id).await?;
    Ok(ResponseJson(ApiResponse::success(repos)))
}

pub async fn create_project_repository(
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateProjectRepository>,
) -> Result<ResponseJson<ApiResponse<ProjectRepository>>, StatusCode> {
    if payload.name.trim().is_empty() {
        return Ok(ResponseJson(ApiResponse::error(
            "Repository name cannot be empty",
        )));
    }

    let CreateProjectRepository {
        name,
        git_repo_path,
        root_path,
        is_primary,
    } = payload;

    let expanded_path = expand_tilde(&git_repo_path);
    let absolute_path = match std::path::absolute(&expanded_path) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!("Failed to resolve repository path {}: {}", git_repo_path, e);
            return Ok(ResponseJson(ApiResponse::error(
                "Failed to resolve repository path on disk",
            )));
        }
    };

    if !absolute_path.exists() {
        return Ok(ResponseJson(ApiResponse::error(
            "The specified repository path does not exist",
        )));
    }

    if !absolute_path.is_dir() {
        return Ok(ResponseJson(ApiResponse::error(
            "The specified repository path is not a directory",
        )));
    }

    if !absolute_path.join(".git").exists() {
        return Ok(ResponseJson(ApiResponse::error(
            "The specified directory is not a git repository",
        )));
    }

    let sanitized_root = root_path.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    if let Some(root) = sanitized_root.as_ref() {
        let relative_root = Path::new(root);
        if relative_root.is_absolute()
            || relative_root
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
        {
            return Ok(ResponseJson(ApiResponse::error(
                "Repository root path must be relative to the repository",
            )));
        }

        let candidate = absolute_path.join(relative_root);
        if !candidate.exists() {
            return Ok(ResponseJson(ApiResponse::error(
                "The specified root path does not exist within the repository",
            )));
        }
    }

    let request = CreateProjectRepository {
        name,
        git_repo_path: absolute_path.to_string_lossy().to_string(),
        root_path: sanitized_root,
        is_primary,
    };

    match ProjectRepository::create(&deployment.db().pool, project.id, &request).await {
        Ok(repository) => Ok(ResponseJson(ApiResponse::success(repository))),
        Err(ProjectRepositoryError::DuplicateName) => Ok(ResponseJson(ApiResponse::error(
            "A repository with this name already exists for this project",
        ))),
        Err(ProjectRepositoryError::DuplicatePath) => Ok(ResponseJson(ApiResponse::error(
            "This repository path and root are already connected to the project",
        ))),
        Err(ProjectRepositoryError::Validation(message)) => {
            Ok(ResponseJson(ApiResponse::error(&message)))
        }
        Err(ProjectRepositoryError::PrimaryRequired) => Ok(ResponseJson(ApiResponse::error(
            "At least one primary repository must remain configured",
        ))),
        Err(ProjectRepositoryError::NotFound) => Err(StatusCode::NOT_FOUND),
        Err(ProjectRepositoryError::Database(err)) => {
            tracing::error!(
                "Failed to create project repository for project {}: {}",
                project.id,
                err
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn update_project_repository(
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
    AxumPath(repo_id): AxumPath<Uuid>,
    Json(mut payload): Json<UpdateProjectRepository>,
) -> Result<ResponseJson<ApiResponse<ProjectRepository>>, StatusCode> {
    let existing_repo = match ProjectRepository::find_by_id(&deployment.db().pool, repo_id).await {
        Ok(Some(repo)) if repo.project_id == project.id => repo,
        Ok(Some(_)) => return Err(StatusCode::NOT_FOUND),
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!(
                "Failed to load repository {} for project {}: {}",
                repo_id,
                project.id,
                e
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if let Some(name) = payload.name.as_ref() {
        if name.trim().is_empty() {
            return Ok(ResponseJson(ApiResponse::error(
                "Repository name cannot be empty",
            )));
        }
    }

    let mut effective_repo_path = existing_repo.git_repo_path.clone();

    if let Some(path) = payload.git_repo_path.as_mut() {
        let expanded = expand_tilde(path);
        let absolute = match std::path::absolute(&expanded) {
            Ok(value) => value,
            Err(e) => {
                tracing::warn!("Failed to resolve repository path {}: {}", path, e);
                return Ok(ResponseJson(ApiResponse::error(
                    "Failed to resolve repository path on disk",
                )));
            }
        };

        if !absolute.exists() {
            return Ok(ResponseJson(ApiResponse::error(
                "The specified repository path does not exist",
            )));
        }

        if !absolute.is_dir() {
            return Ok(ResponseJson(ApiResponse::error(
                "The specified repository path is not a directory",
            )));
        }

        if !absolute.join(".git").exists() {
            return Ok(ResponseJson(ApiResponse::error(
                "The specified directory is not a git repository",
            )));
        }

        *path = absolute.to_string_lossy().to_string();
        effective_repo_path = Path::new(path.as_str()).to_path_buf();
    }

    if let Some(root) = payload.root_path.as_mut() {
        let trimmed = root.trim();
        if trimmed.is_empty() {
            *root = String::new();
        } else {
            let relative_root = Path::new(trimmed);
            if relative_root.is_absolute()
                || relative_root.components().any(|component| {
                    matches!(component, Component::ParentDir | Component::Prefix(_))
                })
            {
                return Ok(ResponseJson(ApiResponse::error(
                    "Repository root path must be relative to the repository",
                )));
            }

            let candidate = effective_repo_path.join(relative_root);
            if !candidate.exists() {
                return Ok(ResponseJson(ApiResponse::error(
                    "The specified root path does not exist within the repository",
                )));
            }

            *root = trimmed.to_string();
        }
    }

    match ProjectRepository::update(&deployment.db().pool, project.id, repo_id, &payload).await {
        Ok(repository) => Ok(ResponseJson(ApiResponse::success(repository))),
        Err(ProjectRepositoryError::DuplicateName) => Ok(ResponseJson(ApiResponse::error(
            "A repository with this name already exists for this project",
        ))),
        Err(ProjectRepositoryError::DuplicatePath) => Ok(ResponseJson(ApiResponse::error(
            "This repository path and root are already connected to the project",
        ))),
        Err(ProjectRepositoryError::Validation(message)) => {
            Ok(ResponseJson(ApiResponse::error(&message)))
        }
        Err(ProjectRepositoryError::PrimaryRequired) => Ok(ResponseJson(ApiResponse::error(
            "At least one primary repository must remain configured",
        ))),
        Err(ProjectRepositoryError::NotFound) => Err(StatusCode::NOT_FOUND),
        Err(ProjectRepositoryError::Database(err)) => {
            tracing::error!(
                "Failed to update project repository {} for project {}: {}",
                repo_id,
                project.id,
                err
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn delete_project_repository(
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
    AxumPath(repo_id): AxumPath<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, StatusCode> {
    match ProjectRepository::delete(&deployment.db().pool, project.id, repo_id).await {
        Ok(()) => Ok(ResponseJson(ApiResponse::success(()))),
        Err(ProjectRepositoryError::PrimaryRequired) => Ok(ResponseJson(ApiResponse::error(
            "Cannot delete the only primary repository. Promote another repository first.",
        ))),
        Err(ProjectRepositoryError::NotFound) => Err(StatusCode::NOT_FOUND),
        Err(ProjectRepositoryError::Validation(message)) => {
            Ok(ResponseJson(ApiResponse::error(&message)))
        }
        Err(ProjectRepositoryError::DuplicateName) | Err(ProjectRepositoryError::DuplicatePath) => {
            Ok(ResponseJson(ApiResponse::error(
                "Unable to delete repository due to conflicting configuration",
            )))
        }
        Err(ProjectRepositoryError::Database(err)) => {
            tracing::error!(
                "Failed to delete repository {} for project {}: {}",
                repo_id,
                project.id,
                err
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn create_project(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateProject>,
) -> Result<ResponseJson<ApiResponse<Project>>, ApiError> {
    let id = Uuid::new_v4();
    let CreateProject {
        name,
        git_repo_path,
        setup_script,
        dev_script,
        cleanup_script,
        copy_files,
        use_existing_repo,
    } = payload;
    tracing::debug!("Creating project '{}'", name);

    // Validate and setup git repository
    let path = std::path::absolute(expand_tilde(&git_repo_path))?;
    // Check if git repo path is already used by another project
    match Project::find_by_git_repo_path(&deployment.db().pool, path.to_string_lossy().as_ref())
        .await
    {
        Ok(Some(_)) => {
            return Ok(ResponseJson(ApiResponse::error(
                "A project with this git repository path already exists",
            )));
        }
        Ok(None) => {
            // Path is available, continue
        }
        Err(e) => {
            return Err(ProjectError::GitRepoCheckFailed(e.to_string()).into());
        }
    }

    if use_existing_repo {
        // For existing repos, validate that the path exists and is a git repository
        if !path.exists() {
            return Ok(ResponseJson(ApiResponse::error(
                "The specified path does not exist",
            )));
        }

        if !path.is_dir() {
            return Ok(ResponseJson(ApiResponse::error(
                "The specified path is not a directory",
            )));
        }

        if !path.join(".git").exists() {
            return Ok(ResponseJson(ApiResponse::error(
                "The specified directory is not a git repository",
            )));
        }

        // Ensure existing repo has a main branch if it's empty
        if let Err(e) = deployment.git().ensure_main_branch_exists(&path) {
            tracing::error!("Failed to ensure main branch exists: {}", e);
            return Ok(ResponseJson(ApiResponse::error(&format!(
                "Failed to ensure main branch exists: {}",
                e
            ))));
        }
    } else {
        // For new repos, create directory and initialize git

        // Create directory if it doesn't exist
        if !path.exists()
            && let Err(e) = std::fs::create_dir_all(&path)
        {
            tracing::error!("Failed to create directory: {}", e);
            return Ok(ResponseJson(ApiResponse::error(&format!(
                "Failed to create directory: {}",
                e
            ))));
        }

        // Check if it's already a git repo, if not initialize it
        if !path.join(".git").exists()
            && let Err(e) = deployment.git().initialize_repo_with_main_branch(&path)
        {
            tracing::error!("Failed to initialize git repository: {}", e);
            return Ok(ResponseJson(ApiResponse::error(&format!(
                "Failed to initialize git repository: {}",
                e
            ))));
        }
    }

    match Project::create(
        &deployment.db().pool,
        &CreateProject {
            name,
            git_repo_path: path.to_string_lossy().to_string(),
            use_existing_repo,
            setup_script,
            dev_script,
            cleanup_script,
            copy_files,
        },
        id,
    )
    .await
    {
        Ok(project) => {
            // Track project creation event
            deployment
                .track_if_analytics_allowed(
                    "project_created",
                    serde_json::json!({
                        "project_id": project.id.to_string(),
                        "use_existing_repo": use_existing_repo,
                        "has_setup_script": project.setup_script.is_some(),
                        "has_dev_script": project.dev_script.is_some(),
                        "source": "manual",
                    }),
                )
                .await;

            Ok(ResponseJson(ApiResponse::success(project)))
        }
        Err(e) => Err(ProjectError::CreateFailed(e.to_string()).into()),
    }
}

pub async fn update_project(
    Extension(existing_project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<UpdateProject>,
) -> Result<ResponseJson<ApiResponse<Project>>, StatusCode> {
    // Destructure payload to handle field updates.
    // This allows us to treat `None` from the payload as an explicit `null` to clear a field,
    // as the frontend currently sends all fields on update.
    let UpdateProject {
        name,
        git_repo_path,
        setup_script,
        dev_script,
        cleanup_script,
        copy_files,
    } = payload;
    // If git_repo_path is being changed, check if the new path is already used by another project
    let git_repo_path = if let Some(new_git_repo_path) = git_repo_path.map(|s| expand_tilde(&s))
        && new_git_repo_path != existing_project.git_repo_path
    {
        match Project::find_by_git_repo_path_excluding_id(
            &deployment.db().pool,
            new_git_repo_path.to_string_lossy().as_ref(),
            existing_project.id,
        )
        .await
        {
            Ok(Some(_)) => {
                return Ok(ResponseJson(ApiResponse::error(
                    "A project with this git repository path already exists",
                )));
            }
            Ok(None) => new_git_repo_path,
            Err(e) => {
                tracing::error!("Failed to check for existing git repo path: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    } else {
        existing_project.git_repo_path
    };

    match Project::update(
        &deployment.db().pool,
        existing_project.id,
        name.unwrap_or(existing_project.name),
        git_repo_path.to_string_lossy().to_string(),
        setup_script,
        dev_script,
        cleanup_script,
        copy_files,
    )
    .await
    {
        Ok(project) => Ok(ResponseJson(ApiResponse::success(project))),
        Err(e) => {
            tracing::error!("Failed to update project: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn delete_project(
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<()>>, StatusCode> {
    match Project::delete(&deployment.db().pool, project.id).await {
        Ok(rows_affected) => {
            if rows_affected == 0 {
                Err(StatusCode::NOT_FOUND)
            } else {
                Ok(ResponseJson(ApiResponse::success(())))
            }
        }
        Err(e) => {
            tracing::error!("Failed to delete project: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(serde::Deserialize)]
pub struct OpenEditorRequest {
    editor_type: Option<String>,
}

pub async fn open_project_in_editor(
    Extension(project): Extension<Project>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<Option<OpenEditorRequest>>,
) -> Result<ResponseJson<ApiResponse<()>>, StatusCode> {
    let path = project.git_repo_path.to_string_lossy();

    let editor_config = {
        let config = deployment.config().read().await;
        let editor_type_str = payload.as_ref().and_then(|req| req.editor_type.as_deref());
        config.editor.with_override(editor_type_str)
    };

    match editor_config.open_file(&path) {
        Ok(_) => {
            tracing::info!("Opened editor for project {} at path: {}", project.id, path);
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(e) => {
            tracing::error!("Failed to open editor for project {}: {}", project.id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn search_project_files(
    State(deployment): State<DeploymentImpl>,
    Extension(project): Extension<Project>,
    Query(params): Query<ProjectSearchQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<SearchResult>>>, StatusCode> {
    let query = params.search.q.trim();
    let mode = params.search.mode.clone();

    if query.is_empty() {
        return Ok(ResponseJson(ApiResponse::error(
            "Query parameter 'q' is required and cannot be empty",
        )));
    }

    let pool = &deployment.db().pool;
    let (repo_path, repo_root) = if let Some(repo_id) = params.repo_id {
        match ProjectRepository::find_by_id(pool, repo_id).await {
            Ok(Some(repo)) if repo.project_id == project.id => {
                (repo.git_repo_path.clone(), repo.root_path.clone())
            }
            Ok(Some(_)) => {
                return Ok(ResponseJson(ApiResponse::error(
                    "Repository not found for this project",
                )));
            }
            Ok(None) => {
                return Ok(ResponseJson(ApiResponse::error("Repository not found")));
            }
            Err(e) => {
                tracing::error!("Failed to load repository {}: {}", repo_id, e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    } else {
        (project.git_repo_path.clone(), String::new())
    };

    let search_root = if repo_root.is_empty() {
        repo_path.clone()
    } else {
        repo_path.join(&repo_root)
    };

    if !search_root.exists() {
        tracing::warn!(
            "Search root {:?} does not exist for project {}",
            search_root,
            project.id
        );
        return Ok(ResponseJson(ApiResponse::error(
            "Selected repository root does not exist",
        )));
    }

    let file_search_cache = deployment.file_search_cache();

    let results = match file_search_cache
        .search(&search_root, query, mode.clone())
        .await
    {
        Ok(results) => {
            tracing::debug!(
                "Cache hit for repo root {:?}, query: {}, mode: {:?}",
                search_root,
                query,
                mode
            );
            results
        }
        Err(CacheError::Miss) => {
            tracing::debug!(
                "Cache miss for repo root {:?}, query: {}, mode: {:?}",
                search_root,
                query,
                mode
            );
            let root_opt = if repo_root.is_empty() {
                None
            } else {
                Some(repo_root.as_str())
            };
            match search_files_in_repo(&repo_path.to_string_lossy(), root_opt, query, mode).await {
                Ok(results) => results,
                Err(e) => {
                    tracing::error!("Failed to search files: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            }
        }
        Err(CacheError::BuildError(err)) => {
            tracing::error!("Cache build error for repo root {:?}: {}", search_root, err);
            let root_opt = if repo_root.is_empty() {
                None
            } else {
                Some(repo_root.as_str())
            };
            match search_files_in_repo(&repo_path.to_string_lossy(), root_opt, query, mode).await {
                Ok(results) => results,
                Err(e) => {
                    tracing::error!("Failed to search files: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            }
        }
    };

    Ok(ResponseJson(ApiResponse::success(results)))
}

async fn search_files_in_repo(
    repo_path: &str,
    root_path: Option<&str>,
    query: &str,
    mode: SearchMode,
) -> Result<Vec<SearchResult>, Box<dyn std::error::Error + Send + Sync>> {
    let repo_path = Path::new(repo_path);

    if !repo_path.exists() {
        return Err("Repository path does not exist".into());
    }

    let root_dir = if let Some(root) = root_path.filter(|r| !r.is_empty()) {
        repo_path.join(root)
    } else {
        repo_path.to_path_buf()
    };

    if !root_dir.exists() {
        return Err("Repository root does not exist".into());
    }

    let mut results = Vec::new();
    let query_lower = query.to_lowercase();

    let walker = match mode {
        SearchMode::Settings => WalkBuilder::new(&root_dir)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .hidden(false)
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                name != ".git"
                    && name != "node_modules"
                    && name != "target"
                    && name != "dist"
                    && name != "build"
            })
            .build(),
        SearchMode::TaskForm => WalkBuilder::new(&root_dir)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .hidden(false)
            .filter_entry(|entry| entry.file_name().to_string_lossy() != ".git")
            .build(),
    };

    for result in walker {
        let entry = result?;
        let path = entry.path();

        if path == root_dir {
            continue;
        }

        let relative_path = path.strip_prefix(&root_dir)?;
        let relative_path_str = relative_path.to_string_lossy().to_lowercase();

        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if file_name.contains(&query_lower) {
            results.push(SearchResult {
                path: relative_path.to_string_lossy().to_string(),
                is_file: path.is_file(),
                match_type: SearchMatchType::FileName,
            });
        } else if relative_path_str.contains(&query_lower) {
            let match_type = if path
                .parent()
                .and_then(|p| p.file_name())
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default()
                .contains(&query_lower)
            {
                SearchMatchType::DirectoryName
            } else {
                SearchMatchType::FullPath
            };

            results.push(SearchResult {
                path: relative_path.to_string_lossy().to_string(),
                is_file: path.is_file(),
                match_type,
            });
        }
    }

    let file_ranker = FileRanker::new();
    match file_ranker.get_stats(repo_path).await {
        Ok(stats) => file_ranker.rerank(&mut results, &stats),
        Err(e) => {
            tracing::warn!(
                "Failed to get git stats for ranking, using basic sort: {}",
                e
            );
            results.sort_by(|a, b| {
                let priority = |match_type: &SearchMatchType| match match_type {
                    SearchMatchType::FileName => 0,
                    SearchMatchType::DirectoryName => 1,
                    SearchMatchType::FullPath => 2,
                };

                priority(&a.match_type)
                    .cmp(&priority(&b.match_type))
                    .then_with(|| a.path.cmp(&b.path))
            });
        }
    }

    results.truncate(10);

    Ok(results)
}

pub fn router(deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    let project_id_router = Router::new()
        .route(
            "/",
            get(get_project).put(update_project).delete(delete_project),
        )
        .route("/activity_feed", get(activity_feed::get_activity_feed))
        .route("/activity_feed/ws", get(project_activity_feed_ws))
        .route("/branches", get(get_project_branches))
        .route(
            "/repositories",
            get(get_project_repositories).post(create_project_repository),
        )
        .route(
            "/repositories/{repo_id}",
            put(update_project_repository).delete(delete_project_repository),
        )
        .route("/search", get(search_project_files))
        .route("/open-editor", post(open_project_in_editor))
        .layer(from_fn_with_state(
            deployment.clone(),
            load_project_middleware,
        ));

    let projects_router = Router::new()
        .route("/", get(get_projects).post(create_project))
        .nest("/{id}", project_id_router);

    Router::new().nest("/projects", projects_router)
}
