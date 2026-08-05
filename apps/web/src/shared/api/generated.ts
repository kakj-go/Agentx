/**
 * This file was auto-generated from openapi/platform-api.json.
 * Only schema components are emitted because the web client uses its shared request layer.
 * Do not make direct changes to the file.
 */

export interface components {
    schemas: {
        AgentIterationDetail: {
            /** Format: uuid */
            agentRunId: string;
            endedAt?: string | null;
            /** Format: uuid */
            id: string;
            /** Format: int32 */
            iterationIndex: number;
            startedAt: string;
            stateAfterHash?: string | null;
            /** Format: uuid */
            stateArtifactId?: string | null;
            stateBeforeHash: string;
            status: string;
            stopReason?: string | null;
        };
        AgentRunDetail: {
            budget: unknown;
            /** Format: int64 */
            costMicros: number;
            endedAt?: string | null;
            /** Format: uuid */
            id: string;
            /** Format: int64 */
            inputTokens: number;
            /** Format: int32 */
            iterationCount: number;
            /** Format: int32 */
            modelCallCount: number;
            /** Format: uuid */
            nodeExecutionId: string;
            /** Format: int64 */
            outputTokens: number;
            startedAt: string;
            /** Format: uuid */
            stateArtifactId?: string | null;
            stateHash?: string | null;
            status: string;
            stopReason?: string | null;
            /** Format: int32 */
            toolCallCount: number;
        };
        ApiErrorResponse: {
            code: string;
            fieldErrors?: components["schemas"]["FieldError"][];
            message: string;
            /** Format: uuid */
            requestId: string;
        };
        ApiKeyResponse: {
            /** Format: date-time */
            createdAt: string;
            /** Format: uuid */
            familyId: string;
            /** Format: uuid */
            id: string;
            /** Format: date-time */
            lastUsedAt?: string | null;
            name: string;
            prefix: string;
            secret?: string | null;
            status: string;
        };
        ApplicationDeploymentResponse: {
            /** Format: uuid */
            applicationId: string;
            /** Format: date-time */
            createdAt: string;
            /** Format: uuid */
            environmentId: string;
            environmentName: string;
            /** Format: uuid */
            id: string;
            inputSchema: unknown;
            outputSchema: unknown;
            /** Format: int64 */
            sequenceNumber: number;
            sessionVersionPolicy: string;
            status: string;
            /** Format: uuid */
            workflowVersionId: string;
            /** Format: int64 */
            workflowVersionNumber: number;
        };
        ApplicationResponse: {
            /** Format: uuid */
            activeDeploymentId?: string | null;
            /** Format: int64 */
            activeVersionNumber?: number | null;
            description?: string | null;
            /** Format: uuid */
            id: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            slug: string;
            status: string;
            /** Format: date-time */
            updatedAt: string;
            /** Format: int64 */
            version: number;
            visibility: string;
            /** Format: uuid */
            workflowId: string;
            workflowName: string;
        };
        ApprovalActionResponse: {
            actionType: string;
            actorName: string;
            /** Format: uuid */
            actorUserId: string;
            /** Format: date-time */
            createdAt: string;
            fromStatus: string;
            /** Format: uuid */
            id: string;
            input?: unknown;
            toStatus: string;
        };
        ApprovalCandidateResponse: {
            displayName: string;
            /** Format: uuid */
            userId: string;
        };
        ApprovalResponse: {
            /** Format: uuid */
            claimedBy?: string | null;
            claimedByName?: string | null;
            /** Format: date-time */
            createdAt: string;
            /** Format: date-time */
            deadlineAt?: string | null;
            description?: string | null;
            /** Format: uuid */
            executionId: string;
            /** Format: uuid */
            id: string;
            nodeId: string;
            requestPayload?: unknown;
            resumeStatus: string;
            status: string;
            title: string;
            /** Format: int64 */
            version: number;
            /** Format: uuid */
            workflowId: string;
            workflowName: string;
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
        CaseInput: {
            caseKey: string;
            context?: unknown;
            evaluatorOverride?: unknown;
            expectedOutput?: unknown;
            input: unknown;
            name: string;
            tags?: string[];
        };
        ChangePasswordRequest: {
            password: string;
            token: string;
        };
        CheckpointListResponse: {
            items: components["schemas"]["CheckpointResponse"][];
        };
        CheckpointResponse: {
            /** Format: int64 */
            activationCount: number;
            checkpointType: string;
            /** Format: date-time */
            createdAt: string;
            /** Format: int64 */
            deliveryCount: number;
            /** Format: uuid */
            executionId: string;
            /** Format: uuid */
            id: string;
            /** Format: uuid */
            nodeExecutionId?: string | null;
            /** Format: int64 */
            sequenceNumber: number;
            stateHash: string;
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
        CreateApiKeyRequest: {
            name: string;
        };
        CreateApplicationDeploymentRequest: {
            /** Format: uuid */
            environmentId: string;
            inputSchema: unknown;
            outputSchema: unknown;
            sessionVersionPolicy: string;
            /** Format: uuid */
            workflowVersionId: string;
        };
        CreateApplicationRequest: {
            description?: string | null;
            name: string;
            slug: string;
            visibility: string;
            /** Format: uuid */
            workflowId: string;
        };
        CreateCaseRequest: components["schemas"]["CaseInput"] & {
            /** Format: int64 */
            expectedRevision: number;
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
        CreateDatasetRequest: {
            description?: string | null;
            name: string;
            visibility: string;
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
        CreateEvaluationProfileRequest: {
            aggregation?: string;
            description?: string | null;
            name: string;
            passThreshold?: string;
            rules: components["schemas"]["EvaluationRuleInput"][];
            visibility?: string;
        };
        CreateEvaluationRunRequest: {
            /** Format: uuid */
            datasetVersionId: string;
            /** Format: uuid */
            evaluationProfileVersionId: string;
            name: string;
            parameters: unknown;
            visibility?: string;
            /** Format: uuid */
            workflowVersionId: string;
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
        CreateSandboxProfileRequest: components["schemas"]["SandboxProfileVersionInput"] & {
            description?: string | null;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
        };
        CreateScheduleRequest: {
            cronExpression: string;
            input: unknown;
            name: string;
            timezone: string;
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
        CreateWebhookRequest: {
            name: string;
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
        DashboardSummaryResponse: {
            /** Format: int64 */
            costMicrosToday: number;
            /** Format: int64 */
            executionsToday: number;
            /** Format: int64 */
            failedToday: number;
            /** Format: int64 */
            pendingApprovals: number;
            /** Format: int64 */
            runningExecutions: number;
            /** Format: int64 */
            succeededToday: number;
            /** Format: int64 */
            workflowCount: number;
        };
        DatasetResponse: {
            /** Format: int64 */
            caseCount: number;
            description?: string | null;
            /** Format: uuid */
            id: string;
            /** Format: int64 */
            latestVersion?: number | null;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            /** Format: int64 */
            revision: number;
            status: string;
            /** Format: date-time */
            updatedAt: string;
            /** Format: int64 */
            version: number;
            visibility: string;
        };
        DatasetVersionResponse: {
            /** Format: int64 */
            caseCount: number;
            contentHash: string;
            /** Format: date-time */
            createdAt: string;
            /** Format: uuid */
            datasetId: string;
            /** Format: uuid */
            id: string;
            /** Format: int64 */
            sourceRevision: number;
            /** Format: int64 */
            versionNumber: number;
        };
        DebugExecutionRequest: {
            /** Format: int64 */
            expectedRevision: number;
            idempotencyKey?: string | null;
            input?: unknown;
            inputSource?: unknown;
            mode: string;
            overlayIds?: string[];
            sideEffectDecisions?: unknown;
            targetNodeId?: string | null;
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
        DebugOverlayResponse: {
            /** Format: uuid */
            artifactId?: string | null;
            /** Format: uuid */
            id: string;
            kind: string;
            nodeId: string;
            payload: unknown;
            schemaHash?: string | null;
            stale: boolean;
            /** Format: date-time */
            updatedAt: string;
            /** Format: uuid */
            updatedBy: string;
            /** Format: uuid */
            workflowId: string;
        };
        DecideApprovalRequest: {
            input?: unknown;
            /** Format: int64 */
            version: number;
        };
        DeleteCaseRequest: {
            /** Format: int64 */
            expectedRevision: number;
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
            definition: unknown;
            definitionHash: string;
            editorDocument: unknown;
            editorHash: string;
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
        EvaluationProfileResponse: {
            aggregation: string;
            description?: string | null;
            /** Format: uuid */
            id: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            passThreshold: string;
            rules: components["schemas"]["EvaluationRuleResponse"][];
            status: string;
            /** Format: int64 */
            version: number;
            /** Format: uuid */
            versionId: string;
            /** Format: int64 */
            versionNumber: number;
            visibility: string;
        };
        EvaluationReportResponse: {
            metrics: unknown[];
            reportStatus: string;
            results: unknown[];
            run: components["schemas"]["EvaluationRunResponse"];
        };
        EvaluationRuleInput: {
            configuration: unknown;
            evaluatorType: string;
            key: string;
            name: string;
            required?: boolean;
            weight?: string;
        };
        EvaluationRuleResponse: {
            configuration: unknown;
            evaluatorType: string;
            /** Format: uuid */
            id: string;
            key: string;
            name: string;
            required: boolean;
            /** Format: int32 */
            sortOrder: number;
            weight: string;
        };
        EvaluationRunResponse: {
            /** Format: date-time */
            createdAt: string;
            datasetName: string;
            /** Format: uuid */
            datasetVersionId: string;
            /** Format: uuid */
            evaluationProfileVersionId: string;
            /** Format: uuid */
            id: string;
            name: string;
            /** Format: uuid */
            ownerDepartmentId: string;
            parameters: unknown;
            /** Format: int64 */
            resultCount: number;
            status: string;
            visibility: string;
            workflowName: string;
            /** Format: uuid */
            workflowVersionId: string;
        };
        ExecutionCommandResponse: {
            /** Format: uuid */
            executionId: string;
            replayed: boolean;
            status: string;
        };
        ExecutionEventListResponse: {
            items: components["schemas"]["ExecutionEventResponse"][];
            /** Format: int64 */
            nextCursor?: number | null;
        };
        ExecutionEventQuery: {
            /** Format: int64 */
            after?: number | null;
            /** Format: int32 */
            limit?: number | null;
        };
        ExecutionEventResponse: {
            eventType: string;
            /** Format: date-time */
            occurredAt: string;
            /** Format: int64 */
            sequence: number;
            status: string;
            summary: unknown;
        };
        ExecutionResponse: {
            /** Format: uuid */
            callerExecutionId?: string | null;
            /** Format: int64 */
            costMicros: number;
            /** Format: int64 */
            durationMs?: number | null;
            /** Format: date-time */
            endedAt?: string | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            executionType: string;
            /** Format: uuid */
            forkCheckpointId?: string | null;
            /** Format: uuid */
            id: string;
            /** Format: int64 */
            inputTokens: number;
            /** Format: uuid */
            invocationId?: string | null;
            /** Format: int64 */
            outputTokens: number;
            /** Format: uuid */
            parentExecutionId?: string | null;
            /** Format: uuid */
            sessionId?: string | null;
            /** Format: date-time */
            startedAt: string;
            status: string;
            /** Format: uuid */
            traceId: string;
            triggerType: string;
            /** Format: uuid */
            workflowId: string;
            workflowName: string;
            /** Format: uuid */
            workflowVersionId?: string | null;
            /** Format: int64 */
            workflowVersionNumber?: number | null;
        };
        ExpressionPreviewRequest: {
            expression: string;
            input?: unknown;
            itemIndex?: number;
            json?: unknown;
            linkedNodes?: unknown;
            /** Format: int32 */
            runIndex?: number;
        };
        ExpressionPreviewResponse: {
            redacted: boolean;
            value: unknown;
        };
        FieldError: {
            code: string;
            field: string;
            message: string;
        };
        ForkRequest: {
            /** Format: uuid */
            checkpointId: string;
            idempotencyKey?: string | null;
            inputOverrides?: unknown;
            mode: string;
            nodeId?: string | null;
            sideEffectDecisions?: unknown;
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
        IdempotentCommandResponse: {
            accepted: boolean;
            replayed: boolean;
        };
        ImportCasesRequest: {
            content: string;
            /** Format: int64 */
            expectedRevision: number;
            format: string;
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
        LineageResponse: {
            /** Format: uuid */
            deliveryId: string;
            /** Format: int32 */
            sourceItemIndex: number;
            /** Format: uuid */
            sourceNodeExecutionId: string;
            /** Format: int32 */
            sourceOutputIndex: number;
            /** Format: int32 */
            sourceRunIndex: number;
            /** Format: int32 */
            targetItemIndex: number;
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
        NodeAttemptResponse: {
            /** Format: int32 */
            attemptNumber: number;
            /** Format: date-time */
            deadlineAt?: string | null;
            /** Format: date-time */
            endedAt?: string | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            /** Format: uuid */
            id: string;
            /** Format: date-time */
            startedAt?: string | null;
            status: string;
            workerInstanceId?: string | null;
        };
        NodeDefinitionDetail: {
            manifest: unknown;
            manifestHash: string;
            nodeType: string;
            /** Format: int32 */
            version: number;
        };
        NodeDefinitionQuery: {
            category?: string | null;
            /** Format: int32 */
            page?: number | null;
            /** Format: int32 */
            pageSize?: number | null;
            search?: string | null;
        };
        NodeDefinitionSummary: {
            capability: string;
            category: string;
            description: string;
            displayName: string;
            executionStyle: string;
            iconKey: string;
            keywords: string[];
            manifestHash: string;
            nodeType: string;
            /** Format: int32 */
            version: number;
        };
        NodeExecutionListResponse: {
            items: components["schemas"]["NodeExecutionResponse"][];
        };
        NodeExecutionResponse: {
            /** Format: int32 */
            activationSlot: number;
            attempts: components["schemas"]["NodeAttemptResponse"][];
            capability: string;
            /** Format: date-time */
            endedAt?: string | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            /** Format: uuid */
            executionId: string;
            /** Format: int32 */
            generation: number;
            /** Format: uuid */
            id: string;
            input?: unknown;
            /** Format: int32 */
            iterationIndex: number;
            lineage: components["schemas"]["LineageResponse"][];
            nodeId: string;
            nodeName: string;
            nodeType: string;
            /** Format: int32 */
            nodeVersion: number;
            output?: unknown;
            /** Format: int32 */
            runIndex: number;
            sideEffectLevel: string;
            /** Format: date-time */
            startedAt?: string | null;
            status: string;
        };
        NodeProviderOption: {
            description?: string | null;
            label: string;
            value: string;
        };
        NodeProviderOptionsResponse: {
            items: components["schemas"]["NodeProviderOption"][];
        };
        NodeProviderQuery: {
            /** Format: int32 */
            limit?: number | null;
            search?: string | null;
        };
        NotificationInboxResponse: {
            items: components["schemas"]["NotificationResponse"][];
            /** Format: int64 */
            unreadCount: number;
        };
        NotificationResponse: {
            arguments: unknown;
            bodyKey: string;
            /** Format: date-time */
            createdAt: string;
            /** Format: uuid */
            id: string;
            notificationType: string;
            read: boolean;
            /** Format: uuid */
            targetId: string;
            targetPath: string;
            targetType: string;
            titleKey: string;
            tone: string;
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
        ReassignApprovalRequest: {
            /** Format: uuid */
            targetUserId: string;
            /** Format: int64 */
            version: number;
        };
        ResourceValidationResponse: {
            missingGrants: components["schemas"]["MissingGrantResponse"][];
            valid: boolean;
        };
        RevisionResponse: {
            /** Format: date-time */
            createdAt: string;
            /** Format: uuid */
            createdBy: string;
            definitionHash: string;
            editorHash: string;
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
        RuntimeCallDetail: {
            /** Format: uuid */
            agentRunId?: string | null;
            /** Format: uuid */
            attemptId: string;
            /** Format: int32 */
            callIndex: number;
            callKind: string;
            /** Format: int64 */
            costMicros: number;
            endedAt?: string | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            /** Format: uuid */
            id: string;
            /** Format: int64 */
            inputTokens: number;
            /** Format: int32 */
            iterationIndex: number;
            /** Format: int64 */
            outputTokens: number;
            requestFingerprint: string;
            /** Format: uuid */
            resourceId?: string | null;
            resourceType?: string | null;
            /** Format: uuid */
            resourceVersionId?: string | null;
            /** Format: uuid */
            responseArtifactId?: string | null;
            sideEffect: string;
            startedAt: string;
            status: string;
            usageEstimated: boolean;
        };
        RuntimeComponentStatus: {
            component: string;
            /** Format: int64 */
            instances?: number | null;
            lastHeartbeat?: string | null;
            /** Format: int64 */
            queueDepth?: number | null;
            status: string;
        };
        RuntimeDetailsResponse: {
            agentRuns: components["schemas"]["AgentRunDetail"][];
            calls: components["schemas"]["RuntimeCallDetail"][];
            /** Format: int64 */
            costMicros: number;
            /** Format: uuid */
            executionId: string;
            /** Format: int64 */
            inputTokens: number;
            iterations: components["schemas"]["AgentIterationDetail"][];
            /** Format: int64 */
            outputTokens: number;
            sandboxes: components["schemas"]["SandboxLeaseDetail"][];
        };
        RuntimeStatusResponse: {
            /** Format: int64 */
            activeSandboxes: number;
            components: components["schemas"]["RuntimeComponentStatus"][];
            /** Format: int64 */
            failedToday: number;
            /** Format: int64 */
            running: number;
            sandboxCompatibility?: unknown;
            /** Format: int64 */
            waiting: number;
        };
        SandboxLeaseDetail: {
            /** Format: uuid */
            attemptId: string;
            createdAt: string;
            expiresAt: string;
            heartbeatAt: string;
            /** Format: uuid */
            id: string;
            lastError?: string | null;
            /** Format: uuid */
            nodeExecutionId: string;
            /** Format: uuid */
            profileVersionId: string;
            sandboxId?: string | null;
            status: string;
            terminatedAt?: string | null;
            /** Format: int32 */
            terminationAttempts: number;
        };
        SandboxProfileResponse: {
            current: components["schemas"]["SandboxProfileVersionResponse"];
            /** Format: int64 */
            currentVersionNumber: number;
            description?: string | null;
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
            versions: components["schemas"]["SandboxProfileVersionResponse"][];
        };
        SandboxProfileVersionInput: {
            /** Format: int32 */
            cpuMillis: number;
            /** Format: int64 */
            diskBytes: number;
            imageDigest: string;
            /** Format: int64 */
            memoryBytes: number;
            networkPolicy: unknown;
            /** Format: int64 */
            outputLimitBytes: number;
            /** Format: int32 */
            pidsLimit: number;
            runner: string;
            /** Format: int32 */
            timeoutSeconds: number;
        };
        SandboxProfileVersionResponse: {
            configurationHash: string;
            /** Format: int32 */
            cpuMillis: number;
            /** Format: date-time */
            createdAt: string;
            /** Format: int64 */
            diskBytes: number;
            /** Format: uuid */
            id: string;
            imageDigest: string;
            /** Format: int64 */
            memoryBytes: number;
            networkPolicy: unknown;
            /** Format: int64 */
            outputLimitBytes: number;
            /** Format: int32 */
            pidsLimit: number;
            runner: string;
            /** Format: int32 */
            timeoutSeconds: number;
            /** Format: int64 */
            versionNumber: number;
        };
        SaveDebugOverlayRequest: {
            /** Format: uuid */
            artifactId?: string | null;
            kind: string;
            payload: unknown;
            schemaHash?: string | null;
        };
        SaveDraftRequest: {
            definition: unknown;
            editorDocument?: unknown;
            /** Format: int64 */
            expectedRevision: number;
        };
        ScheduleResponse: {
            cronExpression: string;
            /** Format: uuid */
            id: string;
            input: unknown;
            name: string;
            status: string;
            timezone: string;
            /** Format: int64 */
            version: number;
        };
        SessionResponse: {
            /** Format: uuid */
            applicationDeploymentId: string;
            /** Format: uuid */
            applicationId: string;
            externalUserId?: string | null;
            /** Format: uuid */
            id: string;
            status: string;
            title?: string | null;
            /** Format: date-time */
            updatedAt: string;
            /** Format: int64 */
            version: number;
            versionPolicy: string;
            /** Format: uuid */
            workflowVersionId?: string | null;
        };
        SideEffectConfirmationRequest: {
            /** Format: uuid */
            checkpointId?: string | null;
            decision: string;
            idempotencyKey: string;
            /** Format: uuid */
            nodeExecutionId: string;
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
        StartExecutionRequest: {
            idempotencyKey?: string | null;
            input?: unknown;
        };
        TestCaseResponse: {
            caseKey: string;
            context?: unknown;
            evaluatorOverride?: unknown;
            expectedOutput?: unknown;
            /** Format: uuid */
            id: string;
            input: unknown;
            name: string;
            /** Format: int64 */
            sortOrder: number;
            tags: string[];
            /** Format: int64 */
            version: number;
        };
        TraceEventResponse: {
            agentRunId?: string | null;
            attemptId?: string | null;
            attributes: unknown;
            contentRef?: string | null;
            /** Format: int64 */
            costMicros: number;
            /** Format: int64 */
            durationMs?: number | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            eventId: string;
            eventTime: string;
            eventType: string;
            executionId: string;
            /** Format: int64 */
            inputTokens?: number | null;
            /** Format: int32 */
            iterationIndex: number;
            mcpToolName?: string | null;
            modelName?: string | null;
            nodeExecutionId?: string | null;
            nodeId?: string | null;
            /** Format: int64 */
            outputTokens?: number | null;
            parentSpanId?: string | null;
            partial: boolean;
            providerName?: string | null;
            resourceId?: string | null;
            resourceType?: string | null;
            resourceVersionId?: string | null;
            /** Format: int32 */
            runIndex: number;
            runtimeCallId?: string | null;
            sandboxId?: string | null;
            spanId: string;
            status: string;
            stopReason?: string | null;
            traceId: string;
        };
        TraceResponse: {
            events: components["schemas"]["TraceEventResponse"][];
            /** Format: uuid */
            executionId: string;
            nextCursor?: string | null;
            /** Format: uuid */
            traceId: string;
        };
        UpdateApplicationRequest: {
            description?: string | null;
            name: string;
            status: string;
            /** Format: int64 */
            version: number;
            visibility: string;
        };
        UpdateCaseRequest: components["schemas"]["CaseInput"] & {
            /** Format: int64 */
            expectedRevision: number;
            /** Format: int64 */
            version: number;
        };
        UpdateCredentialRequest: {
            name: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        UpdateDatasetRequest: {
            description?: string | null;
            name: string;
            status: string;
            /** Format: int64 */
            version: number;
            visibility: string;
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
        UpdateSandboxProfileRequest: {
            description?: string | null;
            name: string;
            status: string;
            /** Format: int64 */
            version: number;
        };
        UpdateScheduleRequest: {
            cronExpression: string;
            input: unknown;
            name: string;
            status: string;
            timezone: string;
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
        UpdateWebhookRequest: {
            name: string;
            status: string;
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
        UpgradeSessionRequest: {
            /** Format: int64 */
            version: number;
            /** Format: uuid */
            workflowVersionId: string;
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
        ValidateDraftRequest: {
            definition: unknown;
            editorDocument?: unknown;
        };
        ValidateDraftResponse: {
            compilerVersion?: string | null;
            definitionHash?: string | null;
            editorHash?: string | null;
            issues: components["schemas"]["ValidationIssue"][];
        };
        ValidationIssue: {
            code: string;
            fieldPath?: string | null;
            message: string;
            nodeId?: string | null;
            severity: string;
        };
        VersionActionRequest: {
            /** Format: int64 */
            version: number;
        };
        WaitListResponse: {
            items: components["schemas"]["WaitResponse"][];
        };
        WaitResponse: {
            authenticationMode: string;
            /** Format: uuid */
            executionId: string;
            /** Format: uuid */
            id: string;
            /** Format: uuid */
            nodeExecutionId: string;
            resumeUrl?: string | null;
            status: string;
            /** Format: date-time */
            timeoutAt?: string | null;
            waitKind: string;
            /** Format: date-time */
            wakeAt?: string | null;
        };
        WebhookResponse: {
            /** Format: uuid */
            id: string;
            name: string;
            path: string;
            publicId: string;
            secret?: string | null;
            status: string;
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
            editorDocument: unknown;
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
