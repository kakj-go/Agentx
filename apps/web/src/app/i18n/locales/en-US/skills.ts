const translations = {
  "uploaded": "File uploaded",
  "details": "Details",
  "publish": "Publish",
  "loading": "Loading…",
  "saved": "Saved",
  "department": "Department",
  "description": "Manage versioned Agent capabilities, runtimes, and Workflow access.",
  "versions": "Versions",
  "createSkill": "New Skill",
  "invalidJson": "Invalid JSON",
  "created": "Created",
  "loadFailed": "Failed to load data",
  "disable": "Disable",
  "enable": "Enable",
  "noVersion": "No version",
  "workspaceRevision": "Workspace revision {{revision}}",
  "workspaceRevisionShort": "Workspace revision",
  "entries": "entries",
  "files": "files",
  "newFolder": "New folder",
  "newMarkdown": "New Markdown",
  "uploadFile": "Upload file",
  "dropUpload": "Drop a file here to upload it into the current folder",
  "selectFile": "Select a file",
  "moveRename": "Move or rename",
  "insertReference": "Insert reference",
  "directorySelected": "Directory selected",
  "fileDetails": "File details",
  "path": "Path",
  "entryType": "Type",
  "mimeType": "MIME type",
  "fileSize": "Size",
  "folderName": "Folder name",
  "markdownName": "Markdown filename",
  "parentFolder": "Parent folder",
  "workspaceRoot": "Workspace root",
  "referenceTarget": "Reference target",
  "publishWorkspace": "Publish workspace version",
  "publishedWorkspace": "Skill version published",
  "editSkill": "Edit Skill",
  "deleteEntry": "Delete entry",
  "deleteEntryConfirm": "Delete {{path}}? The server rejects deletion when the file is referenced.",
  "referencesUpdated": "Move completed and Markdown references were updated",
  "binaryPreview": "This file type displays metadata only",
  "dependencies": "Dependencies JSON",
  "title": "Skills",
  "search": "Search Skills, sources, or runtimes",
  "version": "Version",
  "workflows": "Workflows",
  "workspace": {
    "import": "Import ZIP",
    "export": "Export ZIP",
    "imported": "Workspace imported"
  },
  "deleted": "Deleted",
  "prerequisites": {
    "enableDescription": "Publish an immutable Skill version before enabling it.",
    "version": "At least one published Skill Version",
    "publish": "Publish Skill"
  },
  "markdownEditor": {
    "contentArea": {
      "editableMarkdown": "Editable Markdown"
    },
    "toolbar": {
      "undo": "Undo {{shortcut}}",
      "redo": "Redo {{shortcut}}",
      "blockTypes": {
        "heading": "Heading {{level}}",
        "paragraph": "Paragraph",
        "quote": "Quote"
      },
      "blockTypeSelect": {
        "placeholder": "Block type",
        "selectBlockTypeTooltip": "Select block type"
      },
      "bold": "Bold",
      "removeBold": "Remove bold",
      "italic": "Italic",
      "removeItalic": "Remove italic",
      "underline": "Underline",
      "removeUnderline": "Remove underline",
      "bulletedList": "Bulleted list",
      "numberedList": "Numbered list",
      "checkList": "Check list",
      "link": "Create link",
      "table": "Insert table"
    },
    "dialog": {
      "close": "Close dialog"
    },
    "dialogControls": {
      "cancel": "Cancel",
      "save": "Save"
    },
    "createLink": {
      "cancelTooltip": "Cancel change",
      "saveTooltip": "Set URL",
      "text": "Anchor text",
      "textTooltip": "The text to be displayed for the link",
      "title": "Link title",
      "titleTooltip": "The link's title attribute, shown on hover",
      "url": "URL",
      "urlPlaceholder": "Select or paste a URL"
    },
    "linkPreview": {
      "copied": "Copied!",
      "copyToClipboard": "Copy to clipboard",
      "edit": "Edit link URL",
      "remove": "Remove link"
    },
    "table": {
      "alignCenter": "Align center",
      "alignLeft": "Align left",
      "alignRight": "Align right",
      "columnMenu": "Column menu",
      "deleteColumn": "Delete this column",
      "deleteRow": "Delete this row",
      "deleteTable": "Delete table",
      "insertColumnLeft": "Insert a column to the left of this one",
      "insertColumnRight": "Insert a column to the right of this one",
      "insertRowAbove": "Insert a row above this one",
      "insertRowBelow": "Insert a row below this one",
      "rowMenu": "Row menu",
      "textAlignment": "Text alignment"
    },
    "codeblock": {
      "delete": "Delete code block"
    },
    "codeBlock": {
      "inlineLanguage": "Language",
      "selectLanguage": "Select code block language"
    },
    "imageEditor": {
      "deleteImage": "Delete image",
      "editImage": "Edit image"
    },
    "uploadImage": {
      "addViaUrlInstructions": "Or add an image from a URL:",
      "addViaUrlInstructionsNoUpload": "Add an image from a URL:",
      "alt": "Alt text:",
      "autoCompletePlaceholder": "Select or paste an image source",
      "dialogTitle": "Add an image",
      "height": "Height:",
      "title": "Title:",
      "uploadInstructions": "Upload an image from your device:",
      "width": "Width:"
    }
  },
  "fields": {
    "name": "Skill name",
    "alias": "Skill alias",
    "aliasHint": "Use letters in any language, numbers, hyphens (-), or underscores (_). English letters are normalized to lowercase.",
    "description": "Skill description",
    "descriptionHint": "Used for Skill discovery and identification, and synchronized to the SKILL.md frontmatter.",
    "descriptionPlaceholder": "Describe what this Skill does and when to use it",
    "characterCount": "{{count}} / {{max}}"
  }
} as const

export default translations
