const translations = {
  "uploaded": "文件已上传",
  "details": "详情",
  "publish": "发布",
  "loading": "正在加载…",
  "saved": "已保存",
  "department": "所属部门",
  "description": "管理智能体可加载的可版本化能力、运行方式和工作流授权。",
  "versions": "版本",
  "createSkill": "新建技能",
  "invalidJson": "JSON 格式无效",
  "created": "创建成功",
  "loadFailed": "数据加载失败",
  "disable": "停用",
  "enable": "启用",
  "noVersion": "暂无版本",
  "workspaceRevision": "工作区修订号 {{revision}}",
  "workspaceRevisionShort": "工作区版本",
  "entries": "个条目",
  "files": "文件",
  "newFolder": "新建目录",
  "newMarkdown": "新建 Markdown",
  "uploadFile": "上传文件",
  "dropUpload": "拖拽文件到当前目录即可上传",
  "selectFile": "选择文件",
  "moveRename": "移动或重命名",
  "insertReference": "插入引用",
  "directorySelected": "已选择目录",
  "fileDetails": "文件详情",
  "path": "路径",
  "entryType": "类型",
  "mimeType": "MIME 类型",
  "fileSize": "大小",
  "folderName": "目录名称",
  "markdownName": "Markdown 文件名",
  "parentFolder": "父目录",
  "workspaceRoot": "工作区根目录",
  "referenceTarget": "引用目标",
  "publishWorkspace": "发布工作区版本",
  "publishedWorkspace": "技能版本已发布",
  "editSkill": "编辑技能",
  "deleteEntry": "删除条目",
  "deleteEntryConfirm": "确认删除 {{path}}？被引用的文件会被后端拒绝删除。",
  "referencesUpdated": "移动完成，Markdown 引用已同步更新",
  "binaryPreview": "该文件类型只显示元数据",
  "dependencies": "依赖 JSON",
  "title": "技能",
  "search": "搜索技能、来源或运行方式",
  "version": "版本",
  "workflows": "授权工作流",
  "workspace": {
    "import": "导入 ZIP",
    "export": "导出 ZIP",
    "imported": "工作区已导入"
  },
  "deleted": "删除成功",
  "prerequisites": {
    "enableDescription": "启用技能前需要先发布一个不可变版本。",
    "version": "至少一个已发布的技能版本",
    "publish": "发布技能"
  },
  "markdownEditor": {
    "contentArea": {
      "editableMarkdown": "Markdown 编辑区"
    },
    "toolbar": {
      "undo": "撤销 {{shortcut}}",
      "redo": "重做 {{shortcut}}",
      "blockTypes": {
        "heading": "{{level}} 级标题",
        "paragraph": "正文",
        "quote": "引用"
      },
      "blockTypeSelect": {
        "placeholder": "段落类型",
        "selectBlockTypeTooltip": "选择段落类型"
      },
      "bold": "加粗",
      "removeBold": "取消加粗",
      "italic": "斜体",
      "removeItalic": "取消斜体",
      "underline": "下划线",
      "removeUnderline": "取消下划线",
      "bulletedList": "无序列表",
      "numberedList": "有序列表",
      "checkList": "任务列表",
      "link": "插入链接",
      "table": "插入表格"
    },
    "dialog": {
      "close": "关闭对话框"
    },
    "dialogControls": {
      "cancel": "取消",
      "save": "保存"
    },
    "createLink": {
      "cancelTooltip": "取消修改",
      "saveTooltip": "保存链接",
      "text": "链接文本",
      "textTooltip": "链接中显示的文本",
      "title": "链接标题",
      "titleTooltip": "鼠标悬停时显示的链接标题",
      "url": "URL",
      "urlPlaceholder": "选择或粘贴 URL"
    },
    "linkPreview": {
      "copied": "已复制",
      "copyToClipboard": "复制到剪贴板",
      "edit": "编辑链接 URL",
      "remove": "移除链接"
    },
    "table": {
      "alignCenter": "居中对齐",
      "alignLeft": "左对齐",
      "alignRight": "右对齐",
      "columnMenu": "列菜单",
      "deleteColumn": "删除此列",
      "deleteRow": "删除此行",
      "deleteTable": "删除表格",
      "insertColumnLeft": "在左侧插入一列",
      "insertColumnRight": "在右侧插入一列",
      "insertRowAbove": "在上方插入一行",
      "insertRowBelow": "在下方插入一行",
      "rowMenu": "行菜单",
      "textAlignment": "文本对齐"
    },
    "codeblock": {
      "delete": "删除代码块"
    },
    "codeBlock": {
      "inlineLanguage": "语言",
      "selectLanguage": "选择代码块语言"
    },
    "imageEditor": {
      "deleteImage": "删除图片",
      "editImage": "编辑图片"
    },
    "uploadImage": {
      "addViaUrlInstructions": "或通过 URL 添加图片：",
      "addViaUrlInstructionsNoUpload": "通过 URL 添加图片：",
      "alt": "替代文本：",
      "autoCompletePlaceholder": "选择或粘贴图片地址",
      "dialogTitle": "添加图片",
      "height": "高度：",
      "title": "标题：",
      "uploadInstructions": "从设备上传图片：",
      "width": "宽度："
    }
  },
  "fields": {
    "name": "技能名称",
    "alias": "技能别名",
    "description": "技能描述",
    "descriptionPlaceholder": "说明该技能的用途和适用场景"
  }
} as const

export default translations
