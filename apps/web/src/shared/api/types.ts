import type { components } from './generated'

export type ApiError = components['schemas']['ApiErrorResponse']
export type AuthUser = components['schemas']['MeResponse']
export type AuthResponse = components['schemas']['AuthResponse']
export type Department = components['schemas']['DepartmentResponse']
export type User = components['schemas']['UserResponse']
export type Role = components['schemas']['RoleResponse']
export type Permission = components['schemas']['PermissionResponse']
export type PageResponse<T> = { items: T[]; page: number; pageSize: number; total: number }
