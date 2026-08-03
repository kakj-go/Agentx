/**
 * This file was auto-generated from openapi/platform-api.json.
 * Only schema components are emitted because the web client uses its shared request layer.
 * Do not make direct changes to the file.
 */

export interface components {
    schemas: {
        ApiErrorResponse: {
            code: string;
            fieldErrors?: components["schemas"]["FieldError"][];
            message: string;
            /** Format: uuid */
            requestId: string;
        };
        ArtifactUploadResponse: {
            entry: components["schemas"]["SkillWorkspaceEntry"];
            /** Format: int64 */
            revision: number;
        };
        AuthResponse: {
            accessToken?: string | null;
            changePasswordToken?: string | null;
            /** Format: int64 */
            expiresIn?: number | null;
            passwordChangeRequired: boolean;
            user?: null | components["schemas"]["MeResponse"];
        };
        BootstrapRequest: {
            adminDisplayName: string;
            adminUsername: string;
            companyName: string;
            locale: string;
            password: string;
            timezone: string;
        };
        BootstrapStatus: {
            required: boolean;
        };
        ChangePasswordRequest: {
            password: string;
            token: string;
        };
        ConnectionResponse: {
            configuration: unknown;
            /** Format: uuid */
            credentialId?: string | null;
            endpoint: string;
            healthPath: string;
            /** Format: uuid */
            id: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        CreateConnectionRequest: {
            configuration: unknown;
            /** Format: uuid */
            credentialId?: string | null;
            endpoint: string;
            healthPath?: string | null;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
        };
        CreateCredentialRequest: {
            credentialType: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            secret: unknown;
        };
        CreateDepartmentRequest: {
            name: string;
            /** Format: uuid */
            parentId: string;
        };
        CreateDeploymentRevisionRequest: {
            /** Format: uuid */
            credentialId?: string | null;
            defaultParameters: unknown;
            endpointOverride?: string | null;
            /** Format: int64 */
            expectedAliasVersion: number;
            modelName: string;
            name: string;
            price?: null | components["schemas"]["CreateModelPriceRequest"];
            /** Format: uuid */
            providerId: string;
        };
        CreateEntryRequest: {
            entryType: string;
            /** Format: int64 */
            expectedRevision: number;
            name: string;
            /** Format: uuid */
            parentId?: string | null;
        };
        CreateEnvironmentRequest: {
            code: string;
            name: string;
        };
        CreateGrantRequest: {
            operation: string;
            /** Format: uuid */
            resourceVersionId?: string | null;
            /** Format: uuid */
            subjectId: string;
            subjectType: string;
        };
        CreateKnowledgeRequest: {
            /** Format: uuid */
            connectionId: string;
            externalResourceId: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
        };
        CreateMcpServerRequest: {
            configuration?: unknown;
            /** Format: uuid */
            credentialId?: string | null;
            description?: string | null;
            endpoint: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            transport: string;
        };
        CreateMemoryRequest: {
            accessMode: string;
            /** Format: uuid */
            connectionId: string;
            externalNamespace: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
        };
        CreateModelAliasRequest: {
            alias: string;
            /** Format: uuid */
            deploymentId: string;
        };
        CreateModelDeploymentRequest: {
            /** Format: uuid */
            credentialId?: string | null;
            defaultParameters: unknown;
            endpointOverride?: string | null;
            modelName: string;
            name: string;
            /** Format: uuid */
            providerId: string;
        };
        CreateModelPriceRequest: {
            currency: string;
            inputPerMillion: string;
            outputPerMillion: string;
        };
        CreateModelProviderRequest: {
            /** Format: uuid */
            credentialId?: string | null;
            endpoint: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            providerType: string;
        };
        CreateRoleRequest: {
            code: string;
            dataScope: string;
            description?: string | null;
            name: string;
            permissions: string[];
        };
        CreateSkillRequest: {
            description: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
        };
        CreateUserRequest: {
            /** Format: uuid */
            departmentId: string;
            displayName: string;
            /** Format: uuid */
            roleId: string;
            username: string;
        };
        CreateVersionRequest: {
            /** Format: int64 */
            draftRevision: number;
        };
        CreateWorkflowRequest: {
            description?: string | null;
            name: string;
            visibility: string;
        };
        CredentialResponse: {
            credentialType: string;
            /** Format: int64 */
            currentSecretVersion: number;
            /** Format: uuid */
            id: string;
            maskedHint: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            status: string;
            storageMode: string;
            /** Format: date-time */
            updatedAt: string;
            /** Format: int64 */
            version: number;
        };
        DebugMcpToolRequest: {
            arguments: unknown;
            confirmationText?: string | null;
            confirmed: boolean;
            /** Format: uuid */
            expectedToolVersionId: string;
        };
        DebugMcpToolResponse: {
            /** Format: int64 */
            durationMs: number;
            result: unknown;
            /** Format: uuid */
            toolVersionId: string;
        };
        DepartmentResponse: {
            /** Format: uuid */
            id: string;
            isRoot: boolean;
            name: string;
            /** Format: uuid */
            parentId?: string | null;
            status: string;
            /** Format: int64 */
            version: number;
        };
        DependencyHealth: {
            name: string;
            required: boolean;
            status: string;
        };
        DeploymentResponse: {
            /** Format: date-time */
            createdAt: string;
            /** Format: uuid */
            environmentId: string;
            environmentName: string;
            /** Format: uuid */
            id: string;
            /** Format: int64 */
            sequenceNumber: number;
            source: string;
            status: string;
            /** Format: int64 */
            versionNumber: number;
            /** Format: uuid */
            workflowId: string;
            /** Format: uuid */
            workflowVersionId: string;
        };
        DraftResponse: {
            contentHash: string;
            definition: unknown;
            /** Format: uuid */
            id: string;
            /** Format: int64 */
            revision: number;
            schemaVersion: string;
            /** Format: date-time */
            updatedAt: string;
            /** Format: uuid */
            workflowId: string;
        };
        EnvironmentResponse: {
            code: string;
            /** Format: uuid */
            id: string;
            isBuiltin: boolean;
            name: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        FieldError: {
            code: string;
            field: string;
            message: string;
        };
        GrantResponse: {
            /** Format: date-time */
            createdAt: string;
            /** Format: uuid */
            id: string;
            operation: string;
            /** Format: uuid */
            resourceId: string;
            resourceType: string;
            /** Format: uuid */
            resourceVersionId?: string | null;
            /** Format: uuid */
            subjectId: string;
            subjectType: string;
        };
        GrantableResourceResponse: {
            detail: string;
            /** Format: int64 */
            grantCount: number;
            /** Format: uuid */
            id: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            resourceType: string;
            status: string;
            /** Format: date-time */
            updatedAt: string;
        };
        HealthCheckResponse: {
            /** Format: date-time */
            checkedAt: string;
            errorCode?: string | null;
            errorMessage?: string | null;
            /** Format: int64 */
            latencyMs?: number | null;
            status: string;
        };
        HealthResponse: {
            dependencies?: components["schemas"]["DependencyHealth"][];
            service: string;
            status: string;
            version: string;
        };
        KnowledgeResponse: {
            /** Format: uuid */
            connectionId: string;
            connectionName: string;
            externalResourceId: string;
            /** Format: int64 */
            grantCount: number;
            /** Format: uuid */
            id: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            status: string;
            syncStatus: string;
            /** Format: date-time */
            updatedAt: string;
            /** Format: int64 */
            version: number;
        };
        LoginRequest: {
            password: string;
            username: string;
        };
        McpDiscoveryResponse: {
            /** Format: int32 */
            discoveredCount: number;
            /** Format: uuid */
            runId: string;
            tools: components["schemas"]["McpToolResponse"][];
        };
        McpServerResponse: {
            /** Format: uuid */
            credentialId?: string | null;
            /** Format: int64 */
            currentVersionNumber: number;
            description?: string | null;
            endpoint: string;
            /** Format: uuid */
            id: string;
            /** Format: date-time */
            lastDiscoveredAt?: string | null;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            status: string;
            /** Format: int64 */
            toolCount: number;
            transport: string;
            /** Format: date-time */
            updatedAt: string;
            /** Format: int64 */
            version: number;
        };
        McpToolResponse: {
            annotations: unknown;
            availability: string;
            /** Format: uuid */
            currentVersionId: string;
            /** Format: int64 */
            currentVersionNumber: number;
            debugEnabled: boolean;
            description?: string | null;
            enabled: boolean;
            /** Format: uuid */
            id: string;
            inputSchema: unknown;
            name: string;
            outputSchema?: unknown;
            schemaHash: string;
            /** Format: uuid */
            serverId: string;
            sideEffect: string;
            /** Format: int32 */
            timeoutSeconds: number;
            title?: string | null;
            /** Format: int64 */
            version: number;
        };
        MeResponse: {
            /** Format: uuid */
            companyId: string;
            companyName: string;
            /** Format: uuid */
            departmentId: string;
            departmentName: string;
            displayName: string;
            /** Format: uuid */
            id: string;
            locale: string;
            permissions: string[];
            roles: string[];
            timezone: string;
            username: string;
        };
        MemoryResponse: {
            accessMode: string;
            /** Format: uuid */
            connectionId: string;
            connectionName: string;
            externalNamespace: string;
            /** Format: int64 */
            grantCount: number;
            /** Format: uuid */
            id: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            status: string;
            /** Format: date-time */
            updatedAt: string;
            /** Format: int64 */
            version: number;
        };
        MissingGrantResponse: {
            nodeId: string;
            operation: string;
            reason: string;
            /** Format: uuid */
            requiredByResourceId?: string | null;
            /** Format: uuid */
            resourceId: string;
            resourceType: string;
        };
        ModelDeploymentHistoryResponse: {
            /** Format: date-time */
            changedAt: string;
            /** Format: uuid */
            changedBy: string;
            /** Format: uuid */
            deploymentId: string;
            /** Format: uuid */
            id: string;
            modelName: string;
            /** Format: uuid */
            previousDeploymentId?: string | null;
            /** Format: int64 */
            revisionNumber: number;
        };
        ModelDeploymentResponse: {
            /** Format: uuid */
            credentialId?: string | null;
            defaultParameters: unknown;
            endpointOverride?: string | null;
            /** Format: uuid */
            id: string;
            modelName: string;
            name: string;
            /** Format: uuid */
            providerId: string;
            /** Format: int64 */
            revisionNumber: number;
            status: string;
            /** Format: uuid */
            supersedesDeploymentId?: string | null;
            /** Format: int64 */
            version: number;
        };
        ModelPriceResponse: {
            /** Format: date-time */
            createdAt: string;
            currency: string;
            /** Format: uuid */
            deploymentId: string;
            /** Format: uuid */
            id: string;
            inputPerMillion: string;
            outputPerMillion: string;
            /** Format: int64 */
            versionNumber: number;
        };
        ModelProviderResponse: {
            /** Format: uuid */
            credentialId?: string | null;
            endpoint: string;
            /** Format: uuid */
            id: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            providerType: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        ModelResponse: {
            alias: string;
            /** Format: int64 */
            aliasVersion: number;
            /** Format: date-time */
            connectionCheckedAt?: string | null;
            connectionStatus: string;
            /** Format: uuid */
            deploymentId: string;
            deploymentName: string;
            /** Format: uuid */
            id: string;
            modelName: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            /** Format: uuid */
            providerId: string;
            providerName: string;
            providerType: string;
            status: string;
            /** Format: date-time */
            updatedAt: string;
        };
        MoveEntryRequest: {
            /** Format: int64 */
            expectedRevision: number;
            name: string;
            /** Format: uuid */
            parentId?: string | null;
        };
        PermissionResponse: {
            description?: string | null;
            key: string;
            name: string;
        };
        PublishSkillVersionRequest: {
            dependencies?: components["schemas"]["SkillDependencyInput"][];
            /** Format: int64 */
            expectedRevision: number;
        };
        PublishWorkflowRequest: {
            /** Format: uuid */
            environmentId: string;
            /** Format: uuid */
            workflowVersionId: string;
        };
        ResourceValidationResponse: {
            missingGrants: components["schemas"]["MissingGrantResponse"][];
            valid: boolean;
        };
        RevisionResponse: {
            contentHash: string;
            /** Format: date-time */
            createdAt: string;
            /** Format: uuid */
            createdBy: string;
            /** Format: uuid */
            id: string;
            /** Format: int64 */
            revision: number;
            schemaVersion: string;
        };
        RoleResponse: {
            code: string;
            dataScope: string;
            description?: string | null;
            /** Format: uuid */
            id: string;
            isBuiltin: boolean;
            /** Format: int64 */
            memberCount: number;
            name: string;
            permissions: string[];
            status: string;
            /** Format: int64 */
            version: number;
        };
        RollbackWorkflowRequest: {
            /** Format: uuid */
            targetWorkflowVersionId: string;
        };
        RotateCredentialRequest: {
            secret: unknown;
            /** Format: int64 */
            version: number;
        };
        SaveDraftRequest: {
            definition: unknown;
            /** Format: int64 */
            expectedRevision: number;
        };
        SkillDependencyInput: {
            operation: string;
            /** Format: uuid */
            resourceId: string;
            resourceType: string;
            /** Format: uuid */
            resourceVersionId?: string | null;
        };
        SkillResponse: {
            description?: string | null;
            /** Format: int64 */
            draftRevision: number;
            /** Format: int64 */
            grantCount: number;
            /** Format: uuid */
            id: string;
            /** Format: int64 */
            latestVersion?: number | null;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            status: string;
            /** Format: date-time */
            updatedAt: string;
            /** Format: int64 */
            version: number;
        };
        SkillVersionResponse: {
            contentHash: string;
            /** Format: date-time */
            createdAt: string;
            dependencies: components["schemas"]["SkillDependencyInput"][];
            /** Format: int64 */
            fileCount: number;
            /** Format: uuid */
            id: string;
            manifest: unknown;
            /** Format: uuid */
            skillId: string;
            /** Format: int64 */
            sourceRevision: number;
            /** Format: int64 */
            versionNumber: number;
        };
        SkillWorkspaceEntry: {
            /** Format: uuid */
            artifactId?: string | null;
            contentHash?: string | null;
            editable: boolean;
            entryType: string;
            /** Format: uuid */
            id: string;
            mimeType?: string | null;
            name: string;
            /** Format: uuid */
            parentId?: string | null;
            path: string;
            /** Format: int64 */
            sizeBytes: number;
            /** Format: date-time */
            updatedAt: string;
        };
        SkillWorkspaceResponse: {
            entries: components["schemas"]["SkillWorkspaceEntry"][];
            /** Format: int64 */
            revision: number;
        };
        UpdateCredentialRequest: {
            name: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        UpdateDepartmentRequest: {
            name: string;
            /** Format: uuid */
            parentId?: string | null;
            /** Format: int64 */
            version: number;
        };
        UpdateEnvironmentRequest: {
            name: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        UpdateExternalResourceRequest: {
            name: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        UpdateMarkdownRequest: {
            content: string;
            description?: string | null;
            /** Format: int64 */
            expectedRevision: number;
        };
        UpdateMcpServerRequest: {
            configuration?: unknown;
            /** Format: uuid */
            credentialId?: string | null;
            description?: string | null;
            endpoint: string;
            name: string;
            status: string;
            transport: string;
            /** Format: int64 */
            version: number;
        };
        UpdateMcpToolPolicyRequest: {
            debugEnabled: boolean;
            enabled: boolean;
            sideEffect: string;
            /** Format: int32 */
            timeoutSeconds: number;
            /** Format: int64 */
            version: number;
        };
        UpdateModelAliasRequest: {
            alias: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        UpdateModelProviderRequest: {
            /** Format: uuid */
            credentialId?: string | null;
            endpoint: string;
            name: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        UpdateRoleRequest: {
            dataScope: string;
            description?: string | null;
            name: string;
            permissions: string[];
            /** Format: int64 */
            version: number;
        };
        UpdateSkillRequest: {
            description?: string | null;
            name: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        UpdateUserRequest: {
            /** Format: uuid */
            departmentId: string;
            displayName: string;
            /** Format: uuid */
            roleId: string;
            /** Format: int64 */
            version: number;
        };
        UpdateWorkflowRequest: {
            description?: string | null;
            name: string;
            /** Format: int64 */
            version: number;
            visibility: string;
        };
        UpsertWorkflowMemberRequest: {
            memberRole: string;
            /** Format: uuid */
            userId: string;
        };
        UserResponse: {
            /** Format: uuid */
            departmentId: string;
            departmentName: string;
            displayName: string;
            /** Format: uuid */
            id: string;
            passwordChangeRequired: boolean;
            roles: string[];
            status: string;
            username: string;
            /** Format: int64 */
            version: number;
        };
        WorkflowMemberResponse: {
            displayName: string;
            memberRole: string;
            /** Format: uuid */
            userId: string;
            username: string;
        };
        WorkflowResponse: {
            description?: string | null;
            /** Format: int64 */
            draftRevision: number;
            /** Format: uuid */
            id: string;
            /** Format: int64 */
            latestVersion?: number | null;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            ownerName: string;
            /** Format: uuid */
            ownerUserId: string;
            /** Format: uuid */
            serviceIdentityId: string;
            status: string;
            /** Format: date-time */
            updatedAt: string;
            /** Format: int64 */
            version: number;
            visibility: string;
        };
        WorkflowVersionResponse: {
            contentHash: string;
            /** Format: date-time */
            createdAt: string;
            /** Format: uuid */
            createdBy: string;
            definition: unknown;
            /** Format: uuid */
            id: string;
            schemaVersion: string;
            /** Format: int64 */
            sourceRevision: number;
            /** Format: int64 */
            versionNumber: number;
            /** Format: uuid */
            workflowId: string;
        };
    };
    responses: never;
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
