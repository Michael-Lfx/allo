const zhCN = {
  nav: {
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
    releases: "发布",
    github: "GitHub",
    copyright: "© Flowy Agent Store 贡献者。本站点与运行时以本地优先为原则。",
  },
  landing: {
    eyebrow: "本地优先 · 单文件运行时",
    heroTitle: "一个可执行文件，装下你的 Agent 工作台",
    heroSubtitle:
      "Flowy Agent Store 把 Agent 运行时打包成单个可执行文件，内嵌完整 Web UI。用命令行启动，浏览器即刻打开操作台——无需服务器，数据留在本地。",
    heroCtaDownload: "下载",
    heroCtaDocs: "阅读文档",
    featureTitle: "为本地工作台而生",
    featureSubtitle: "把专家、团队、技能与连接器收敛进一个可信的本地进程。",
    features: {
      singleBinary: {
        title: "单文件运行时",
        desc: "一个可执行文件包含全部能力，无依赖安装、无容器，下载即可运行。",
      },
      localFirst: {
        title: "本地优先",
        desc: "执行、凭据与运行状态都在本机；云端只负责定义、版本与分发。",
      },
      catalog: {
        title: "Agent 目录",
        desc: "统一管理 Agent、Team、Skill 与 Connector，导入即可查询与运行。",
      },
      cliUi: {
        title: "命令行启动 Web UI",
        desc: "一条命令拉起本地 App Server，浏览器打开即可编排与观测运行。",
      },
    },
    workflowTitle: "命令行 → 浏览器，三步上手",
    workflowSubtitle: "启动、连接、操作，全部在本机完成。",
    workflow: {
      step1: {
        title: "启动运行时",
        desc: "在终端运行命令，单文件可执行程序在本地拉起 App Server。",
        cmd: "flowy-agent-store serve",
      },
      step2: {
        title: "本地 App Server",
        desc: "进程在 localhost 提供版本化协议，UI 只经由 SDK 与之通信。",
        cmd: "http://localhost:8787",
      },
      step3: {
        title: "打开 Web UI",
        desc: "浏览器自动打开工作台，导入 Agent 并启动单次或团队运行。",
        cmd: "在浏览器中操作",
      },
    },
    downloadTitle: "下载 Flowy Agent Store",
    downloadSubtitle: "选择你的平台，或在发布页查看全部构建。",
    download: {
      primaryCta: "下载 {{os}}",
      detectNote: "已根据你当前的系统识别平台",
      allPlatforms: "全部平台",
      manual: "手动选择平台",
      releaseNote: "查看 GitHub Releases 获取历史版本与校验和",
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
