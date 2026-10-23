//! 团队 Command：现阶段团队仅作为项目的容器（schema 预留成员/权限扩展）。

use tauri::State;
use uuid::Uuid;

use miyo_core::model::Team;
use miyo_storage::repository as repo;

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// 列出全部团队（默认团队恒在首位，由迁移 0015 保证存在）。
#[tauri::command(rename_all = "camelCase")]
pub async fn list_teams(state: State<'_, AppState>) -> CommandResult<Vec<Team>> {
    repo::list_teams(&state.db).await.map_err(Into::into)
}

/// 新建团队。
#[tauri::command(rename_all = "camelCase")]
pub async fn create_team(state: State<'_, AppState>, name: String) -> CommandResult<Team> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CommandError::validation("团队名称不能为空"));
    }
    repo::create_team(&state.db, name).await.map_err(Into::into)
}

/// 重命名团队。
#[tauri::command(rename_all = "camelCase")]
pub async fn update_team(state: State<'_, AppState>, team: Team) -> CommandResult<Team> {
    if team.name.trim().is_empty() {
        return Err(CommandError::validation("团队名称不能为空"));
    }
    repo::update_team(&state.db, &team)
        .await
        .map_err(Into::into)
}

/// 删除团队：有下属项目时拒绝（错误信息提示先迁移）。
#[tauri::command(rename_all = "camelCase")]
pub async fn delete_team(state: State<'_, AppState>, team_id: Uuid) -> CommandResult<()> {
    repo::delete_team(&state.db, team_id)
        .await
        .map_err(Into::into)
}

/// 项目移入团队（团队须存在）。
#[tauri::command(rename_all = "camelCase")]
pub async fn move_project_to_team(
    state: State<'_, AppState>,
    project_id: Uuid,
    team_id: Uuid,
) -> CommandResult<()> {
    repo::move_project_to_team(&state.db, project_id, team_id).await?;
    // 激活项目被移动：刷新缓存，前端读到新 team_id。
    if let Ok(p) = repo::get_project(&state.db, project_id).await {
        state.refresh_active_project(p).await;
    }
    Ok(())
}
