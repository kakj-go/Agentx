/** Generated schema components from openapi/platform-api.json. Do not edit. */
export interface components {
    schemas: {
        AgentIterationDetail: {
            agentRunId: string;
            endedAt?: string | null;
            id: string;
            iterationIndex: number;
            startedAt: string;
            stateAfterHash?: string | null;
            stateArtifactId?: string | null;
            stateBeforeHash: string;
            status: string;
            stopReason?: string | null;
        };
        AgentRunDetail: {
            budget: unknown;
            costMicros: number;
            endedAt?: string | null;
            id: string;
            inputTokens: number;
            iterationCount: number;
            modelCallCount: number;
            nodeExecutionId: string;
            outputTokens: number;
            startedAt: string;
            stateArtifactId?: string | null;
            stateHash?: string | null;
            status: string;
            stopReason?: string | null;
            toolCallCount: number;
        };
        ApiErrorResponse: {
            code: string;
            details?: unknown;
            fieldErrors?: components["schemas"]["FieldError"][];
            message: string;
            requestId: string;
        };
        ApiKeyResponse: {
            createdAt: string;
            familyId: string;
            id: string;
            lastUsedAt?: string | null;
            name: string;
            prefix: string;
            secret?: string | null;
            status: string;
        };
        ApplicationDeploymentResponse: {
            applicationId: string;
            createdAt: string;
            environmentId: string;
            environmentName: string;
            id: string;
            inputSchema: unknown;
            outputSchema: unknown;
            publishAttemptId?: string | null;
            publishErrorCode?: string | null;
            publishErrorMessage?: string | null;
            sequenceNumber: number;
            sessionVersionPolicy: string;
            status: string;
            workflowVersionId: string;
            workflowVersionNumber: number;
        };
        ApplicationResponse: {
            activeDeploymentId?: string | null;
            activeVersionNumber?: number | null;
            description?: string | null;
            id: string;
            name: string;
            ownerDepartmentId: string;
            publishedRuntimeConfigRevision: number;
            runtimeConfigRevision: number;
            slug: string;
            status: string;
            updatedAt: string;
            version: number;
            visibility: string;
            workflowId: string;
            workflowName: string;
        };
        ApprovalActionResponse: {
            actionType: string;
            actorName: string;
            actorUserId: string;
            createdAt: string;
            fromStatus: string;
            id: string;
            input?: unknown;
            toStatus: string;
        };
        ApprovalCandidateResponse: {
            displayName: string;
            userId: string;
        };
        ApprovalResponse: {
            claimedBy?: string | null;
            claimedByName?: string | null;
            createdAt: string;
            deadlineAt?: string | null;
            description?: string | null;
            executionId: string;
            id: string;
            nodeId: string;
            requestPayload?: unknown;
            resumeStatus: string;
            status: string;
            title: string;
            version: number;
            workflowId: string;
            workflowName: string;
        };
        ArtifactUploadResponse: {
            entry: components["schemas"]["SkillWorkspaceEntry"];
            revision: number;
        };
        AuthResponse: {
            accessToken?: string | null;
            changePasswordToken?: string | null;
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
        CancelResourceGrantRequest: {
            expectedVersion: number;
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
            activationCount: number;
            checkpointType: string;
            createdAt: string;
            deliveryCount: number;
            executionId: string;
            id: string;
            nodeExecutionId?: string | null;
            sequenceNumber: number;
            stateHash: string;
        };
        ConnectionResponse: {
            configuration: unknown;
            credentialId?: string | null;
            endpoint: string;
            healthPath: string;
            id: string;
            name: string;
            ownerDepartmentId: string;
            status: string;
            version: number;
        };
        CreateApiKeyRequest: {
            name: string;
        };
        CreateApplicationDeploymentRequest: {
            environmentId: string;
            sessionVersionPolicy: string;
            workflowVersionId: string;
        };
        CreateApplicationRequest: {
            description?: string | null;
            name: string;
            slug: string;
            visibility: string;
            workflowId: string;
        };
        CreateCaseRequest: components["schemas"]["CaseInput"] & {
            expectedRevision: number;
        };
        CreateConnectionRequest: {
            configuration: unknown;
            credentialId?: string | null;
            endpoint: string;
            healthPath?: string | null;
            name: string;
            ownerDepartmentId: string;
        };
        CreateCredentialRequest: {
            credentialType: string;
            name: string;
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
            parentId: string;
        };
        CreateEntryRequest: {
            entryType: string;
            expectedRevision: number;
            name: string;
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
            datasetVersionId: string;
            evaluationProfileVersionId: string;
            name: string;
            parameters: unknown;
            visibility?: string;
            workflowVersionId: string;
        };
        CreateGrantRequest: {
            operation: string;
            resourceVersionId?: string | null;
            subjectId: string;
            subjectType: string;
        };
        CreateKnowledgeRequest: {
            connectionId: string;
            externalResourceId: string;
            name: string;
            ownerDepartmentId: string;
        };
        CreateMcpServerRequest: {
            configuration?: unknown;
            credentialId?: string | null;
            description?: string | null;
            endpoint: string;
            name: string;
            ownerDepartmentId: string;
            transport: string;
        };
        CreateMemoryRequest: {
            accessMode: string;
            connectionId: string;
            externalNamespace: string;
            name: string;
            ownerDepartmentId: string;
        };
        CreateModelPriceRequest: {
            currency: string;
            inputPerMillion: string;
            outputPerMillion: string;
        };
        CreateModelRequest: {
            alias?: string;
            connectionName: string;
            credentialId?: string | null;
            defaultParameters?: unknown;
            endpoint: string;
            maxInputTokens?: number;
            maxOutputTokens?: number;
            modelName?: string;
            ownerDepartmentId: string;
            price?: null | components["schemas"]["CreateModelPriceRequest"];
            providerType: string;
        };
        CreateResourceGrantRequest: {
            message?: string | null;
            operation: string;
            resourceId: string;
            resourceType: string;
            resourceVersionId?: string | null;
            sourceNodeId?: string | null;
            sourceRevision?: number | null;
        };
        CreateRetentionRunRequest: {
            artifactRetentionDays?: number;
            dryRun?: boolean;
            evaluationRetentionDays?: number;
            messageRetentionDays?: number;
            traceRetentionDays?: number;
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
            ownerDepartmentId: string;
        };
        CreateScheduleRequest: {
            cronExpression: string;
            input: unknown;
            misfirePolicy?: string;
            name: string;
            timezone: string;
        };
        CreateSkillRequest: {
            alias: string;
            description: string;
            name: string;
            ownerDepartmentId: string;
        };
        CreateUserRequest: {
            departmentId: string;
            displayName: string;
            roleId: string;
            username: string;
        };
        CreateVersionRequest: {
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
            currentSecretVersion: number;
            id: string;
            maskedHint: string;
            name: string;
            ownerDepartmentId: string;
            status: string;
            storageMode: string;
            updatedAt: string;
            version: number;
        };
        DashboardSummaryResponse: {
            costMicrosToday: number;
            executionsToday: number;
            failedToday: number;
            pendingApprovals: number;
            runningExecutions: number;
            succeededToday: number;
            workflowCount: number;
        };
        DatasetResponse: {
            caseCount: number;
            description?: string | null;
            id: string;
            latestVersion?: number | null;
            name: string;
            ownerDepartmentId: string;
            revision: number;
            status: string;
            updatedAt: string;
            version: number;
            visibility: string;
        };
        DatasetVersionResponse: {
            caseCount: number;
            contentHash: string;
            createdAt: string;
            datasetId: string;
            id: string;
            sourceRevision: number;
            versionNumber: number;
        };
        DebugExecutionRequest: {
            context?: unknown;
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
            expectedToolVersionId: string;
        };
        DebugMcpToolResponse: {
            durationMs: number;
            result: unknown;
            toolVersionId: string;
        };
        DebugOverlayResponse: {
            artifactId?: string | null;
            id: string;
            kind: string;
            nodeId: string;
            payload: unknown;
            schemaHash?: string | null;
            stale: boolean;
            updatedAt: string;
            updatedBy: string;
            workflowId: string;
        };
        DecideApprovalRequest: {
            input?: unknown;
            version: number;
        };
        DeleteCaseRequest: {
            expectedRevision: number;
        };
        DeletionImpactResponse: {
            deletable: boolean;
            page: number;
            pageSize: number;
            references: components["schemas"]["DeletionReference"][];
            targetVersion: number;
            total: number;
        };
        DeletionReference: {
            immutable: boolean;
            nodeId?: string | null;
            nodeName?: string | null;
            parentId?: string | null;
            relation: string;
            sourceId: string;
            sourceModule: string;
            sourceName: string;
            sourceType: string;
        };
        DepartmentResponse: {
            id: string;
            isRoot: boolean;
            name: string;
            parentId?: string | null;
            status: string;
            version: number;
        };
        DependencyHealth: {
            name: string;
            required: boolean;
            status: string;
        };
        DeploymentResponse: {
            createdAt: string;
            environmentId: string;
            environmentName: string;
            id: string;
            sequenceNumber: number;
            source: string;
            status: string;
            versionNumber: number;
            workflowId: string;
            workflowVersionId: string;
        };
        DraftResponse: {
            definition: unknown;
            definitionHash: string;
            editorDocument: unknown;
            editorHash: string;
            id: string;
            revision: number;
            schemaVersion: string;
            updatedAt: string;
            workflowId: string;
        };
        EnvironmentResponse: {
            code: string;
            id: string;
            isBuiltin: boolean;
            name: string;
            status: string;
            version: number;
        };
        EvaluationCaseResultResponse: {
            caseId: string;
            caseKey: string;
            costMicros: number;
            detail?: unknown;
            durationMs?: number | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            ruleResults: components["schemas"]["EvaluationRuleResultResponse"][];
            score?: number | null;
            sourceCaseId: string;
            status: string;
            targetExecutionId?: string | null;
        };
        EvaluationProfileResponse: {
            aggregation: string;
            description?: string | null;
            id: string;
            name: string;
            ownerDepartmentId: string;
            passThreshold: string;
            rules: components["schemas"]["EvaluationRuleResponse"][];
            status: string;
            version: number;
            versionId: string;
            versionNumber: number;
            visibility: string;
        };
        EvaluationReportResponse: {
            metrics: unknown[];
            reportStatus: string;
            results: components["schemas"]["EvaluationCaseResultResponse"][];
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
            id: string;
            key: string;
            name: string;
            required: boolean;
            sortOrder: number;
            weight: string;
        };
        EvaluationRuleResultResponse: {
            costMicros: number;
            detail: unknown;
            durationMs?: number | null;
            evaluatorExecutionId?: string | null;
            evaluatorType: string;
            id: string;
            key: string;
            name: string;
            passed?: boolean | null;
            score?: number | null;
            status: string;
        };
        EvaluationRunResponse: {
            createdAt: string;
            datasetName: string;
            datasetVersionId: string;
            evaluationProfileVersionId: string;
            id: string;
            name: string;
            ownerDepartmentId: string;
            parameters: unknown;
            resultCount: number;
            status: string;
            visibility: string;
            workflowName: string;
            workflowVersionId: string;
        };
        ExecutionCommandResponse: {
            executionId: string;
            replayed: boolean;
            status: string;
        };
        ExecutionEventListResponse: {
            items: components["schemas"]["ExecutionEventResponse"][];
            nextCursor?: number | null;
        };
        ExecutionEventQuery: {
            after?: number | null;
            limit?: number | null;
        };
        ExecutionEventResponse: {
            eventType: string;
            occurredAt: string;
            sequence: number;
            status: string;
            summary: unknown;
        };
        ExecutionResponse: {
            callerExecutionId?: string | null;
            costMicros: number;
            durationMs?: number | null;
            endedAt?: string | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            executionType: string;
            forkCheckpointId?: string | null;
            id: string;
            inputTokens: number;
            invocationId?: string | null;
            outputTokens: number;
            parentExecutionId?: string | null;
            sessionId?: string | null;
            startedAt: string;
            status: string;
            traceId: string;
            triggerType: string;
            workflowId: string;
            workflowName: string;
            workflowVersionId?: string | null;
            workflowVersionNumber?: number | null;
        };
        ExpressionPreviewRequest: {
            expression: string;
            input?: unknown;
            itemIndex?: number;
            json?: unknown;
            linkedNodes?: unknown;
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
            checkpointId: string;
            idempotencyKey?: string | null;
            inputOverrides?: unknown;
            mode: string;
            nodeId?: string | null;
            sideEffectDecisions?: unknown;
        };
        GrantResponse: {
            createdAt: string;
            id: string;
            operation: string;
            resourceId: string;
            resourceType: string;
            resourceVersionId?: string | null;
            subjectId: string;
            subjectType: string;
        };
        GrantableResourceResponse: {
            detail: string;
            grantCount: number;
            id: string;
            name: string;
            ownerDepartmentId: string;
            resourceType: string;
            status: string;
            updatedAt: string;
        };
        HealthCheckResponse: {
            checkedAt: string;
            errorCode?: string | null;
            errorMessage?: string | null;
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
            expectedRevision: number;
            format: string;
        };
        ImportWorkflowPackageRequest: {
            description?: string | null;
            name: string;
            package: components["schemas"]["WorkflowPackage"];
            resourceBindings?: {
                [key: string]: components["schemas"]["ResourceBindingTarget"];
            };
            visibility: string;
        };
        ImportedWorkflowResponse: {
            definitionHash: string;
            draftId: string;
            workflowId: string;
        };
        KnowledgeResponse: {
            connectionId: string;
            connectionName: string;
            externalResourceId: string;
            grantCount: number;
            id: string;
            name: string;
            ownerDepartmentId: string;
            status: string;
            syncStatus: string;
            updatedAt: string;
            version: number;
        };
        LineageResponse: {
            deliveryId: string;
            sourceItemIndex: number;
            sourceNodeExecutionId: string;
            sourceOutputIndex: number;
            sourceRunIndex: number;
            targetItemIndex: number;
        };
        LoginRequest: {
            password: string;
            username: string;
        };
        McpDiscoveryResponse: {
            discoveredCount: number;
            runId: string;
            tools: components["schemas"]["McpToolResponse"][];
        };
        McpServerResponse: {
            credentialId?: string | null;
            currentVersionNumber: number;
            description?: string | null;
            endpoint: string;
            id: string;
            lastDiscoveredAt?: string | null;
            name: string;
            ownerDepartmentId: string;
            status: string;
            toolCount: number;
            transport: string;
            updatedAt: string;
            version: number;
        };
        McpToolResponse: {
            annotations: unknown;
            availability: string;
            currentVersionId: string;
            currentVersionNumber: number;
            debugEnabled: boolean;
            description?: string | null;
            enabled: boolean;
            id: string;
            inputSchema: unknown;
            name: string;
            outputSchema?: unknown;
            schemaHash: string;
            serverId: string;
            sideEffect: string;
            timeoutSeconds: number;
            title?: string | null;
            version: number;
        };
        MeResponse: {
            companyId: string;
            companyName: string;
            departmentId: string;
            departmentName: string;
            displayName: string;
            id: string;
            locale: string;
            permissions: string[];
            roles: string[];
            timezone: string;
            username: string;
        };
        MemoryResponse: {
            accessMode: string;
            connectionId: string;
            connectionName: string;
            externalNamespace: string;
            grantCount: number;
            id: string;
            name: string;
            ownerDepartmentId: string;
            status: string;
            updatedAt: string;
            version: number;
        };
        MissingGrantResponse: {
            nodeId: string;
            operation: string;
            reason: string;
            requiredByResourceId?: string | null;
            resourceId: string;
            resourceType: string;
        };
        ModelDeploymentHistoryResponse: {
            changedAt: string;
            changedBy: string;
            connectionName: string;
            deploymentId: string;
            id: string;
            modelName: string;
            previousDeploymentId?: string | null;
            revisionNumber: number;
        };
        ModelPriceResponse: {
            createdAt: string;
            currency: string;
            deploymentId: string;
            id: string;
            inputPerMillion: string;
            outputPerMillion: string;
            versionNumber: number;
        };
        ModelResponse: {
            alias: string;
            aliasVersion: number;
            connectionCheckedAt?: string | null;
            connectionName: string;
            connectionStatus: string;
            credentialId?: string | null;
            defaultParameters: unknown;
            deploymentId: string;
            endpoint: string;
            id: string;
            maxInputTokens: number;
            maxOutputTokens: number;
            modelName: string;
            ownerDepartmentId: string;
            providerType: string;
            revisionNumber: number;
            status: string;
            updatedAt: string;
        };
        MoveEntryRequest: {
            expectedRevision: number;
            name: string;
            parentId?: string | null;
        };
        NodeAttemptResponse: {
            attemptNumber: number;
            deadlineAt?: string | null;
            endedAt?: string | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            id: string;
            startedAt?: string | null;
            status: string;
            workerInstanceId?: string | null;
        };
        NodeDefinitionDetail: {
            manifest: unknown;
            manifestHash: string;
            nodeType: string;
            version: number;
        };
        NodeDefinitionQuery: {
            category?: string | null;
            page?: number | null;
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
            version: number;
        };
        NodeExecutionListResponse: {
            items: components["schemas"]["NodeExecutionResponse"][];
        };
        NodeExecutionResponse: {
            activationSlot: number;
            attempts: components["schemas"]["NodeAttemptResponse"][];
            capability: string;
            endedAt?: string | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            executionId: string;
            generation: number;
            id: string;
            input?: unknown;
            iterationIndex: number;
            lineage: components["schemas"]["LineageResponse"][];
            nodeId: string;
            nodeName: string;
            nodeType: string;
            nodeVersion: number;
            output?: unknown;
            runIndex: number;
            sideEffectLevel: string;
            startedAt?: string | null;
            status: string;
        };
        NodeLock: {
            manifestHash: string;
            nodeType: string;
            version: number;
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
            limit?: number | null;
            search?: string | null;
        };
        NotificationInboxResponse: {
            items: components["schemas"]["NotificationResponse"][];
            unreadCount: number;
        };
        NotificationResponse: {
            arguments: unknown;
            bodyKey: string;
            createdAt: string;
            id: string;
            notificationType: string;
            read: boolean;
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
        PublishAttemptResponse: {
            activationSequence: number;
            bundleId?: string | null;
            deploymentId: string;
            errorCode?: string | null;
            errorMessage?: string | null;
            id: string;
            nextAction: string;
            previousAttemptId?: string | null;
            state: string;
            updatedAt: string;
        };
        PublishSkillVersionRequest: {
            dependencies?: components["schemas"]["SkillDependencyInput"][];
            expectedRevision: number;
        };
        PublishWorkflowRequest: {
            environmentId: string;
            workflowVersionId: string;
        };
        QueuedWorkflowRunResponse: {
            commandId: string;
            status: string;
        };
        QuotaPolicyInput: {
            dimension: string;
            hardLimit: string;
            periodSeconds?: number | null;
        };
        QuotaPolicyResponse: {
            activeReserved: string;
            dimension: string;
            hardLimit: string;
            periodSeconds?: number | null;
            periodUsage: string;
            version: number;
        };
        ReassignApprovalRequest: {
            targetUserId: string;
            version: number;
        };
        ResourceAuthorizationInput: {
            operation: string;
            resourceId: string;
            resourceType: string;
            resourceVersionId?: string | null;
        };
        ResourceAuthorizationResponse: {
            alreadyGrantedCount: number;
            grantedCount: number;
            resourceId: string;
            resourceType: string;
            workflowId: string;
        };
        ResourceBindingPlaceholder: {
            bindingRole?: string | null;
            id: string;
            nodeId: string;
            operation: string;
            referenceIndex: number;
            resourceType: string;
        };
        ResourceBindingTarget: {
            resourceId: string;
            resourceVersionId?: string | null;
        };
        ResourceGrantRequestAuditResponse: {
            action: string;
            actorName?: string | null;
            id: string;
            occurredAt: string;
        };
        ResourceGrantRequestResponse: {
            createdAt: string;
            history: components["schemas"]["ResourceGrantRequestAuditResponse"][];
            id: string;
            items: components["schemas"]["ResourceRequirementResponse"][];
            message?: string | null;
            operation: string;
            primaryResourceId: string;
            primaryResourceName?: string | null;
            primaryResourceType: string;
            requestedBy: string;
            requestedByName: string;
            reviews: components["schemas"]["ResourceGrantRequestReviewResponse"][];
            sourceNodeId?: string | null;
            sourceRevision?: number | null;
            status: string;
            updatedAt: string;
            version: number;
            workflowId: string;
            workflowName: string;
            workflowServiceIdentityId: string;
        };
        ResourceGrantRequestReviewResponse: {
            canAct: boolean;
            id: string;
            ownerDepartmentId: string;
            ownerDepartmentName: string;
            reviewComment?: string | null;
            reviewedAt?: string | null;
            reviewedBy?: string | null;
            reviewedByName?: string | null;
            status: string;
            version: number;
        };
        ResourceOptionQuery: {
            operation?: string | null;
            page?: number | null;
            pageSize?: number | null;
            resourceType: string;
            search?: string | null;
        };
        ResourceRequirementResponse: {
            active: boolean;
            authorized: boolean;
            name?: string | null;
            operation: string;
            ownerDepartmentId?: string | null;
            requiredByResourceId?: string | null;
            resourceId: string;
            resourceType: string;
            resourceVersionId?: string | null;
        };
        ResourceValidationResponse: {
            missingGrants: components["schemas"]["MissingGrantResponse"][];
            valid: boolean;
        };
        RetentionItemResponse: {
            attemptCount: number;
            dataType: string;
            id: string;
            reason?: string | null;
            status: string;
            targetId: string;
            updatedAt: string;
        };
        RetentionRunResponse: {
            candidateCount: number;
            completedAt?: string | null;
            createdAt: string;
            deletedCount: number;
            dryRun: boolean;
            errorMessage?: string | null;
            id: string;
            status: string;
        };
        ReviewResourceGrantRequest: {
            comment?: string | null;
            expectedVersion: number;
        };
        RevisionResponse: {
            createdAt: string;
            createdBy: string;
            definitionHash: string;
            editorHash: string;
            id: string;
            revision: number;
            schemaVersion: string;
        };
        RoleResponse: {
            code: string;
            dataScope: string;
            description?: string | null;
            id: string;
            isBuiltin: boolean;
            memberCount: number;
            name: string;
            permissions: string[];
            status: string;
            version: number;
        };
        RollbackWorkflowRequest: {
            targetWorkflowVersionId: string;
        };
        RotateCredentialRequest: {
            secret: unknown;
            version: number;
        };
        RunWorkflowRequest: {
            idempotencyKey?: string | null;
            input?: unknown;
        };
        RuntimeCallDetail: {
            agentRunId?: string | null;
            attemptId: string;
            callIndex: number;
            callKind: string;
            costMicros: number;
            endedAt?: string | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            id: string;
            inputTokens: number;
            iterationIndex: number;
            outputTokens: number;
            requestFingerprint: string;
            resourceId?: string | null;
            resourceType?: string | null;
            resourceVersionId?: string | null;
            responseArtifactId?: string | null;
            sideEffect: string;
            startedAt: string;
            status: string;
            usageEstimated: boolean;
        };
        RuntimeComponentStatus: {
            component: string;
            instances?: number | null;
            lastHeartbeat?: string | null;
            queueDepth?: number | null;
            status: string;
        };
        RuntimeDetailsResponse: {
            agentRuns: components["schemas"]["AgentRunDetail"][];
            calls: components["schemas"]["RuntimeCallDetail"][];
            costMicros: number;
            executionId: string;
            inputTokens: number;
            iterations: components["schemas"]["AgentIterationDetail"][];
            outputTokens: number;
            sandboxes: components["schemas"]["SandboxLeaseDetail"][];
        };
        RuntimeStatusResponse: {
            activeSandboxes: number;
            components: components["schemas"]["RuntimeComponentStatus"][];
            failedToday: number;
            running: number;
            sandboxCompatibility?: unknown;
            waiting: number;
        };
        SandboxLeaseDetail: {
            attemptId: string;
            createdAt: string;
            expiresAt: string;
            heartbeatAt: string;
            id: string;
            lastError?: string | null;
            nodeExecutionId: string;
            profileVersionId: string;
            sandboxId?: string | null;
            status: string;
            terminatedAt?: string | null;
            terminationAttempts: number;
        };
        SandboxProfileResponse: {
            current: components["schemas"]["SandboxProfileVersionResponse"];
            currentVersionNumber: number;
            description?: string | null;
            id: string;
            name: string;
            ownerDepartmentId: string;
            status: string;
            updatedAt: string;
            version: number;
            versions: components["schemas"]["SandboxProfileVersionResponse"][];
        };
        SandboxProfileVersionInput: {
            cpuMillis: number;
            diskBytes: number;
            imageDigest: string;
            memoryBytes: number;
            networkPolicy: unknown;
            outputLimitBytes: number;
            pidsLimit: number;
            runner: string;
            timeoutSeconds: number;
        };
        SandboxProfileVersionResponse: {
            configurationHash: string;
            cpuMillis: number;
            createdAt: string;
            diskBytes: number;
            id: string;
            imageDigest: string;
            memoryBytes: number;
            networkPolicy: unknown;
            outputLimitBytes: number;
            pidsLimit: number;
            runner: string;
            timeoutSeconds: number;
            versionNumber: number;
        };
        SaveDebugOverlayRequest: {
            artifactId?: string | null;
            kind: string;
            payload: unknown;
            schemaHash?: string | null;
        };
        SaveDraftRequest: {
            definition: unknown;
            editorDocument?: unknown;
            expectedRevision: number;
        };
        ScheduleResponse: {
            cronExpression: string;
            id: string;
            input: unknown;
            misfirePolicy: string;
            name: string;
            status: string;
            timezone: string;
            version: number;
        };
        SessionResponse: {
            applicationDeploymentId: string;
            applicationId: string;
            externalUserId?: string | null;
            id: string;
            status: string;
            title?: string | null;
            updatedAt: string;
            version: number;
            versionPolicy: string;
            workflowVersionId?: string | null;
        };
        SideEffectConfirmationRequest: {
            checkpointId?: string | null;
            decision: string;
            idempotencyKey: string;
            nodeExecutionId: string;
        };
        SkillDependencyInput: {
            operation: string;
            resourceId: string;
            resourceType: string;
            resourceVersionId?: string | null;
        };
        SkillResponse: {
            alias: string;
            description?: string | null;
            draftRevision: number;
            grantCount: number;
            id: string;
            latestVersion?: number | null;
            name: string;
            ownerDepartmentId: string;
            status: string;
            updatedAt: string;
            version: number;
        };
        SkillVersionResponse: {
            contentHash: string;
            createdAt: string;
            dependencies: components["schemas"]["SkillDependencyInput"][];
            fileCount: number;
            id: string;
            manifest: unknown;
            skillId: string;
            sourceRevision: number;
            versionNumber: number;
        };
        SkillWorkspaceEntry: {
            artifactId?: string | null;
            contentHash?: string | null;
            editable: boolean;
            entryType: string;
            id: string;
            mimeType?: string | null;
            name: string;
            parentId?: string | null;
            path: string;
            sizeBytes: number;
            updatedAt: string;
        };
        SkillWorkspaceResponse: {
            entries: components["schemas"]["SkillWorkspaceEntry"][];
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
            id: string;
            input: unknown;
            name: string;
            sortOrder: number;
            tags: string[];
            version: number;
        };
        TraceEventResponse: {
            agentRunId?: string | null;
            attemptId?: string | null;
            attributes: unknown;
            contentRef?: string | null;
            costMicros: number;
            durationMs?: number | null;
            errorCode?: string | null;
            errorMessage?: string | null;
            eventId: string;
            eventTime: string;
            eventType: string;
            executionId: string;
            inputTokens?: number | null;
            iterationIndex: number;
            mcpToolName?: string | null;
            modelName?: string | null;
            nodeExecutionId?: string | null;
            nodeId?: string | null;
            outputTokens?: number | null;
            parentSpanId?: string | null;
            partial: boolean;
            providerName?: string | null;
            resourceId?: string | null;
            resourceType?: string | null;
            resourceVersionId?: string | null;
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
            executionId: string;
            nextCursor?: string | null;
            traceId: string;
        };
        UpdateApplicationRequest: {
            description?: string | null;
            name: string;
            status: string;
            version: number;
            visibility: string;
        };
        UpdateCaseRequest: components["schemas"]["CaseInput"] & {
            expectedRevision: number;
            version: number;
        };
        UpdateCredentialRequest: {
            name: string;
            status: string;
            version: number;
        };
        UpdateDatasetRequest: {
            description?: string | null;
            name: string;
            status: string;
            version: number;
            visibility: string;
        };
        UpdateDepartmentRequest: {
            name: string;
            parentId?: string | null;
            version: number;
        };
        UpdateEnvironmentRequest: {
            name: string;
            status: string;
            version: number;
        };
        UpdateExternalResourceRequest: {
            name: string;
            status: string;
            version: number;
        };
        UpdateMarkdownRequest: {
            content: string;
            description?: string | null;
            expectedRevision: number;
        };
        UpdateMcpServerRequest: {
            configuration?: unknown;
            credentialId?: string | null;
            description?: string | null;
            endpoint: string;
            name: string;
            status: string;
            transport: string;
            version: number;
        };
        UpdateMcpToolPolicyRequest: {
            debugEnabled: boolean;
            enabled: boolean;
            sideEffect: string;
            timeoutSeconds: number;
            version: number;
        };
        UpdateModelRequest: {
            alias: string;
            connectionName: string;
            credentialId?: string | null;
            defaultParameters: unknown;
            endpoint: string;
            expectedAliasVersion: number;
            maxInputTokens: number;
            maxOutputTokens: number;
            modelName: string;
            ownerDepartmentId: string;
            price?: null | components["schemas"]["CreateModelPriceRequest"];
            providerType: string;
            status: string;
        };
        UpdateQuotaPoliciesRequest: {
            policies: components["schemas"]["QuotaPolicyInput"][];
        };
        UpdateRoleRequest: {
            dataScope: string;
            description?: string | null;
            name: string;
            permissions: string[];
            version: number;
        };
        UpdateSandboxProfileRequest: {
            description?: string | null;
            name: string;
            status: string;
            version: number;
        };
        UpdateScheduleRequest: {
            cronExpression: string;
            input: unknown;
            misfirePolicy?: string;
            name: string;
            status: string;
            timezone: string;
            version: number;
        };
        UpdateSkillRequest: {
            alias: string;
            description?: string | null;
            name: string;
            status: string;
            version: number;
        };
        UpdateUserRequest: {
            departmentId: string;
            displayName: string;
            roleId: string;
            version: number;
        };
        UpdateWebhookRequest: {
            name: string;
            status: string;
            version: number;
        };
        UpdateWorkflowRequest: {
            description?: string | null;
            name: string;
            version: number;
            visibility: string;
        };
        UpgradeSessionRequest: {
            version: number;
            workflowVersionId: string;
        };
        UpsertWorkflowMemberRequest: {
            memberRole: string;
            userId: string;
        };
        UserResponse: {
            departmentId: string;
            departmentName: string;
            displayName: string;
            id: string;
            passwordChangeRequired: boolean;
            roles: string[];
            status: string;
            username: string;
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
            version: number;
        };
        WaitListResponse: {
            items: components["schemas"]["WaitResponse"][];
        };
        WaitResponse: {
            authenticationMode: string;
            executionId: string;
            id: string;
            nodeExecutionId: string;
            resumeUrl?: string | null;
            status: string;
            timeoutAt?: string | null;
            waitKind: string;
            wakeAt?: string | null;
        };
        WebhookResponse: {
            id: string;
            name: string;
            path: string;
            publicId: string;
            secret?: string | null;
            status: string;
            version: number;
        };
        WorkerCapabilityResponse: {
            capability: string;
            compilerVersionMax: string;
            compilerVersionMin: string;
            heartbeatAt: string;
            instanceId: string;
            irSchemaVersions: unknown;
            manifestHashes: unknown;
            nodeProtocolVersion: string;
            status: string;
        };
        WorkflowMemberResponse: {
            displayName: string;
            memberRole: string;
            userId: string;
            username: string;
        };
        WorkflowPackage: {
            apiBinding: unknown;
            editorDocument: unknown;
            manifest: components["schemas"]["WorkflowPackageManifest"];
            nodeLock: components["schemas"]["NodeLock"][];
            resourceBindingPlaceholders: components["schemas"]["ResourceBindingPlaceholder"][];
            subWorkflowReferences: string[];
            workflowDefinition: unknown;
        };
        WorkflowPackageManifest: {
            contentHash: string;
            name: string;
            packageSchemaVersion: string;
            signature: string;
            signatureAlgorithm: string;
            signingKeyId: string;
            workflowSchemaVersion: string;
        };
        WorkflowResourceOptionResponse: {
            accessState: string;
            detail: string;
            id: string;
            name: string;
            pendingRequestId?: string | null;
            requirements: components["schemas"]["ResourceRequirementResponse"][];
            resourceType: string;
            resourceVersionId?: string | null;
            status: string;
        };
        WorkflowResponse: {
            description?: string | null;
            draftRevision: number;
            id: string;
            latestVersion?: number | null;
            name: string;
            ownerDepartmentId: string;
            ownerName: string;
            ownerUserId: string;
            serviceIdentityId: string;
            status: string;
            updatedAt: string;
            version: number;
            visibility: string;
        };
        WorkflowVersionResponse: {
            contentHash: string;
            createdAt: string;
            createdBy: string;
            definition: unknown;
            editorDocument: unknown;
            id: string;
            schemaVersion: string;
            sourceRevision: number;
            versionNumber: number;
            workflowId: string;
        };
    };
    responses: never;
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
