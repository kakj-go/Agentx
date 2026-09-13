const translations = {
  title: 'Canvas Plugins',
  description: 'Import and manage custom Workflow nodes, interactive UI, and execution logic.',
  import: 'Import plugin', template: 'Download developer template', search: 'Search plugins', details: 'Details',
  fields: { name: 'Name', packageId: 'Package ID', versions: 'Versions', nodes: 'Nodes', updatedAt: 'Updated', status: 'Status', source: 'Source' },
  sources: { builtin: 'Built-in', imported: 'Imported' }, enabled: 'Enabled', disabled: 'Disabled',
  importTitle: 'Import canvas plugin', chooseFile: 'Choose a .agentx-plugin file', validating: 'Validating plugin package…',
  confirmImport: 'Import plugin', imported: 'Plugin imported',
  enableAfterImport: 'Enable after import', defaultAfterImport: 'Use as the default for new nodes',
  versions: 'Versions', drafts: 'Drafts', deployments: 'Deployments', executionArtifacts: 'Frozen execution artifacts', usage: 'Usage', development: 'Development', audit: 'Audit',
  enable: 'Enable', disable: 'Disable', setDefault: 'Set default', defaultVersion: 'Default version',
  downloadVersion: 'Download plugin version', deleteVersion: 'Delete plugin version',
  uninstall: 'Uninstall plugin', referenceCount: 'Total references',
  confirmDelete: 'Delete this version? The server rejects the operation when references exist.', confirmUninstall: 'Uninstall this plugin?',
  templateHint: 'The template includes SDKs, AGENTS.md, field-level protocols, and build commands.', builtinHint: 'This built-in package uses the public plugin execution contract and ships with Agentx.', loadFailed: 'Could not load canvas plugins.',
} as const
export default translations
