const zhCN = {
  nav: {
    market: "市场",
    docs: "文档",
    download: "下载",
    github: "GitHub",
    themeLight: "浅色",
    themeDark: "深色",
    lang: "语言",
  },
  footer: {
    tagline: "本地优先的单文件 Agent 运行时。",
    docs: "文档",
    resources: "资源",
    releases: "发布",
    github: "GitHub",
    copyright: "© Flowy Agent Store 贡献者。本站点与运行时以本地优先为原则。",
  },
  landing: {
    eyebrow: "本地优先 · 单文件运行时",
    heroTitle: "本地优先的 Agent 工作台",
    heroSubtitle:
      "单个可执行文件，浏览器打开即用。导入专家、组建团队、编排运行——执行、凭据与运行状态只留本机，云端仅同步定义与版本。",
    heroCtaDownload: "下载",
    heroCtaMarket: "浏览市场资源",
    heroCtaDocs: "阅读文档",
    heroTerminalListening: "App Server 已在 http://127.0.0.1:8787 启动",
    heroTerminalOpened: "工作台已在浏览器中打开",
    showcase: {
      label: "产品预览",
      runTitle: "Team Run · frontend-backend-experts",
      status: "运行中",
      navCatalog: "目录",
      navRuns: "运行",
      navArtifacts: "产物",
      tlPlan: "计划已生成 · 4 步 DAG",
      tlTool: "工具调用 · browser.open",
      tlArt: "产物 · todo-api.ts",
    },
    featureTitle: "为本地工作台而生",
    featureSubtitle: "导入、运行、观测——全部在一个可信的本地进程里完成。",
    features: {
      workbench: {
        title: "本地 Agent 目录",
        desc: "专家、技能、连接器统一导入与检索，转为不可变快照，导入即可查询与运行。",
      },
      observability: {
        title: "运行全程可观测",
        desc: "计划（DAG）、事件时间线与产物实时呈现；单 Agent 与团队运行都可回放。",
      },
      localFirst: {
        title: "本地优先执行",
        desc: "凭据与运行状态不出本机；云端只负责市场目录、版本与分发的同步。",
      },
      oneCmd: {
        title: "一条命令开工",
        desc: "无数据库、无常驻服务；flowy-agent-store 即起，浏览器打开即用。",
      },
    },
    workflowTitle: "命令行 → 工作台，三步上手",
    workflowSubtitle: "下载、导入、运行，全部在本机完成。",
    workflow: {
      step1: {
        title: "启动运行时",
        desc: "运行命令，单文件在本地拉起 App Server，并自动打开工作台。",
        cmd: "flowy-agent-store",
      },
      step2: {
        title: "导入专家",
        desc: "从工作台导入 CodeBuddy / WorkBuddy 插件，或用 SDK 调 installStoreEntry()。",
        cmd: "导入 CodeBuddy / WorkBuddy 插件",
      },
      step3: {
        title: "运行与观测",
        desc: "选择 Agent 或团队运行，实时查看 DAG、事件时间线与产物。",
        cmd: "Run → DAG · Timeline · Artifacts",
      },
    },
    marketStrip: {
      title: "市场资源，开箱即用",
      subtitle: "专家、技能与连接器一键导入本地目录，持续更新。",
      cta: "进入市场",
    },
    dev: {
      title: "为开发者而生",
      subtitle:
        "类型安全的 TypeScript SDK：只用类型、连接已运行的 App Server，或一键拉起整个运行时。",
      pkgProtocol: {
        name: "@flowy-agent-store/protocol",
        desc: "线协议唯一类型源：请求、响应、通知与错误。零运行时依赖。",
      },
      pkgClient: {
        name: "@flowy-agent-store/client",
        desc: "AppServerClient 与 7 个子客户端，Transport 抽象可接 WS / HTTP。",
      },
      pkgSdk: {
        name: "@flowy-agent-store/sdk",
        desc: "spawn 二进制 → 回环 WebSocket → 就绪握手，返回可用客户端。",
      },
      codeTitle: "quick-start.ts",
      cta: "阅读 TypeScript SDK 指南",
    },
    faq: {
      title: "常见问题",
      items: {
        q1: {
          q: "本地优先的边界是什么？",
          a: "执行、凭据与运行状态只存在于你的机器；云端仅用于市场目录、版本与分发的同步。没有云端执行，也不上传运行数据。",
        },
        q2: {
          q: "支持哪些平台？",
          a: "兼容性矩阵覆盖 macOS（Apple 芯片 / Intel）、Windows x64 与 Linux x64 / arm64。当前发布渠道优先提供 Windows x64 构建，其余平台随构建发布逐步开放。",
        },
        q3: {
          q: "需要登录吗？",
          a: "本机默认免登录（本地可信模式）。开放到局域网（--host 0.0.0.0）时建议开启 --auth，管理员账号在首次启动时创建。",
        },
        q4: {
          q: "能导入哪些来源？",
          a: "CodeBuddy 插件与 WorkBuddy 技能 / 连接器经导入器转为不可变快照；导入后即可在目录中查询与运行。未确认版权的资源不会进入公开分发。",
        },
      },
    },
    downloadTitle: "下载 Flowy Agent Store",
    downloadSubtitle: "选择你的平台，或在发布页查看全部构建。",
    download: {
      primaryCta: "下载 {{os}}",
      detectNote: "已根据你当前的系统识别平台",
      allPlatforms: "全部平台",
      manual: "手动选择平台",
      releaseNote: "查看下载中心获取历史版本与校验和",
      copy: "复制命令",
      copied: "已复制",
      fallbackCta: "前往 Releases",
    },
    platforms: {
      macos: "macOS",
      windows: "Windows",
      linux: "Linux",
      archAarch64: "Apple 芯片",
      archX8664: "Intel / x64",
    },
    marketStat: "市场收录资源",
  },
  market: {
    title: "资源市场",
    subtitle: "专家、技能与连接器，一处浏览、搜索即得。",
    updated: "数据更新于 {{date}}。",
    searchPlaceholder: "搜索名称、描述或标签…",
    tabs: {
      experts: "专家",
      skills: "技能",
      connectors: "连接器",
    },
    empty: "没有匹配的资源。",
    emptyHint: "换个关键词，或切换分类再试。",
  },
  docs: {
    title: "文档",
    subtitle: "快速开始、命令行用法、架构与兼容性。",
    onThisPage: "本页目录",
    backToDocs: "返回文档",
    notFound: "未找到该文档",
    notFoundBody: "请检查链接，或返回文档首页。",
    sections: {
      quickStart: "快速开始",
      cli: "命令行用法",
      pluginsMarket: "插件与市场",
      typescriptSdk: "TypeScript SDK",
      configuration: "配置文件",
      architecture: "架构说明",
      compatibility: "兼容性矩阵",
    },
  },
  common: {
    notFound: "页面不存在",
    home: "返回首页",
  },
};

export type Resources = typeof zhCN;
export default zhCN;
