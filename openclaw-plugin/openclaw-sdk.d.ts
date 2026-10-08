// Type declarations for OpenClaw plugin SDK (peer dependency provided at runtime)

declare module "openclaw/plugin-sdk/tool-plugin" {
  import { TSchema, Static } from "@sinclair/typebox";

  export type ToolPluginExecutionContext = {
    api: unknown;
    signal?: AbortSignal;
    toolCallId: string;
    onUpdate?: (update: unknown) => void;
  };

  export type ToolPluginToolFactory<TConfig> = <TParamsSchema extends TSchema>(
    definition: {
      name: string;
      label?: string;
      description: string;
      parameters: TParamsSchema;
      optional?: boolean;
      execute: (params: Static<TParamsSchema>, config: TConfig, context: ToolPluginExecutionContext) => unknown;
    }
  ) => {
    name: string;
    label: string;
    description: string;
    parameters: TSchema;
    optional: boolean;
    execute?: (params: unknown, config: unknown, context: ToolPluginExecutionContext) => unknown;
  };

  export type DefineToolPluginOptions<TConfigSchema extends TSchema | undefined = undefined> = {
    id: string;
    name: string;
    description: string;
    activation?: { onStartup?: boolean };
    configSchema?: TConfigSchema;
    tools: (tool: ToolPluginToolFactory<TConfigSchema extends TSchema ? Static<TConfigSchema> : Record<string, never>>) => readonly unknown[];
  };

  export function defineToolPlugin<TConfigSchema extends TSchema | undefined = undefined>(
    definition: DefineToolPluginOptions<TConfigSchema>
  ): unknown;
}
