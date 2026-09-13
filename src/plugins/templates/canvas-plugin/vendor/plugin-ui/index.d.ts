import type * as React from 'react';
export type PluginPanelProps = {
    parameters: Record<string, unknown>;
    readOnly: boolean;
    fieldErrors: Record<string, string>;
    providerOptions: Record<string, unknown[]>;
    referenceCatalog?: unknown;
    resources: Record<string, unknown>;
    updateParameters(patch: Record<string, unknown>): void;
};
export type PluginUiHost = {
    React: typeof React;
    locale: string;
    theme: 'light' | 'dark';
    portalRoot: HTMLElement;
    assets: Record<string, string>;
    components: {
        Field: React.ComponentType<{
            label: string;
            error?: string;
            children?: React.ReactNode;
        }>;
        Input: React.ComponentType<React.InputHTMLAttributes<HTMLInputElement>>;
        Button: React.ComponentType<any>;
        Select: React.ComponentType<any>;
        SmartInput: React.ComponentType<any>;
    };
    design?: {
        resolveDefinition(configuration: Record<string, unknown>, upstreamContracts?: Record<string, unknown>, signal?: AbortSignal): Promise<unknown>;
        invokeProvider(provider: string, input?: {
            search?: string;
            limit?: number;
            cursor?: string;
            parameters?: Record<string, unknown>;
        }, signal?: AbortSignal): Promise<{
            items: unknown[];
            nextCursor?: string | null;
        }>;
    };
};
export type PluginUiModule = {
    Panel: React.ComponentType<PluginPanelProps>;
    Canvas?: React.ComponentType<{
        parameters: Record<string, unknown>;
    }>;
    Result?: React.ComponentType<{
        value: unknown;
    }>;
    traceRenderers?: Record<string, React.ComponentType<{
        value: unknown;
    }>>;
};
export type CreatePluginUi = (host: PluginUiHost) => PluginUiModule;
