use agentx_api_types::PageResponse;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapStatus {
    pub required: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapRequest {
    pub company_name: String,
    pub admin_username: String,
    pub admin_display_name: String,
    pub password: String,
    pub locale: String,
    pub timezone: String,
}

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub token: String,
    pub password: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub access_token: Option<String>,
    pub expires_in: Option<i64>,
    pub password_change_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_password_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<MeResponse>,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub company_id: Uuid,
    pub company_name: String,
    pub department_id: Uuid,
    pub department_name: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub locale: String,
    pub timezone: String,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DepartmentResponse {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub is_root: bool,
    pub status: String,
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDepartmentRequest {
    pub parent_id: Uuid,
    pub name: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDepartmentRequest {
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub version: u64,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub status: String,
    pub password_change_required: bool,
    pub department_id: Uuid,
    pub department_name: String,
    pub roles: Vec<String>,
    pub version: u64,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub display_name: String,
    pub department_id: Uuid,
    pub role_id: Uuid,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    pub display_name: String,
    pub department_id: Uuid,
    pub role_id: Uuid,
    pub version: u64,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleResponse {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub data_scope: String,
    pub is_builtin: bool,
    pub status: String,
    pub permissions: Vec<String>,
    pub member_count: u64,
    pub version: u64,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    pub key: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleRequest {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub data_scope: String,
    pub permissions: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoleRequest {
    pub name: String,
    pub description: Option<String>,
    pub data_scope: String,
    pub permissions: Vec<String>,
    pub version: u64,
}

pub type UserPage = PageResponse<UserResponse>;
pub type RolePage = PageResponse<RoleResponse>;
