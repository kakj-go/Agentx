/**
 * Auto-generated from openapi/trigger-gateway.json. Do not edit directly.
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
            /** Format: uuid */
            artifactId: string;
            contentType: string;
            /** Format: int64 */
            sizeBytes: number;
        };
        CreateSessionRequest: {
            externalUserId?: string | null;
            title?: string | null;
        };
        FieldError: {
            code: string;
            field: string;
            message: string;
        };
        InvocationRequest: {
            input: unknown;
            responseMode?: string | null;
            /** Format: uuid */
            sessionId?: string | null;
        };
        InvocationResponse: {
            /** Format: uuid */
            applicationId: string;
            /** Format: date-time */
            createdAt: string;
            error?: unknown;
            /** Format: uuid */
            executionId?: string | null;
            /** Format: uuid */
            id: string;
            outputs?: unknown;
            /** Format: uuid */
            sessionId?: string | null;
            status: string;
        };
        MessagePartInput: {
            /** Format: uuid */
            artifactId?: string | null;
            content?: unknown;
            partType: string;
        };
        MessagePartResponse: {
            /** Format: uuid */
            artifactId?: string | null;
            content?: unknown;
            partType: string;
        };
        MessageRequest: {
            parts: components["schemas"]["MessagePartInput"][];
        };
        MessageResponse: {
            /** Format: date-time */
            createdAt: string;
            /** Format: uuid */
            id: string;
            /** Format: uuid */
            invocationId?: string | null;
            parts: components["schemas"]["MessagePartResponse"][];
            role: string;
            /** Format: int64 */
            sequence: number;
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
        WaitResumeRequest: {
            outputPort?: string | null;
            payload?: unknown;
        };
        WaitResumeResponse: {
            accepted: boolean;
            replayed: boolean;
        };
    };
    responses: never;
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
