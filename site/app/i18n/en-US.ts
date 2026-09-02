const enUS = {
  nav: {
    docs: "Docs",
    download: "Download",
    github: "GitHub",
    themeLight: "Light",
    themeDark: "Dark",
    lang: "Language",
  },
  footer: {
    tagline: "A local-first, single-file agent runtime.",
    docs: "Docs",
    releases: "Releases",
    github: "GitHub",
    copyright:
      "© Flowy Agent Store contributors. This site and runtime are local-first by design.",
  },
  landing: {
    eyebrow: "Local-first · Single-file runtime",
    heroTitle: "One binary, your whole agent workbench",
    heroSubtitle:
      "Flowy Agent Store packages the agent runtime as a single executable with a full Web UI embedded. Launch it from the command line and the workbench opens in your browser — no server required, your data stays local.",
    heroCtaDownload: "Download",
    heroCtaDocs: "Read the docs",
    featureTitle: "Built for the local workbench",
    featureSubtitle:
      "Bring experts, teams, skills and connectors into one trusted local process.",
    features: {
      singleBinary: {
        title: "Single-file runtime",
        desc: "One executable carries everything — no install step, no containers, run it straight from the download.",
      },
      localFirst: {
        title: "Local-first",
        desc: "Execution, credentials and run state live on your machine; the cloud only handles definitions, versions and distribution.",
      },
      catalog: {
        title: "Agent catalog",
        desc: "Manage Agents, Teams, Skills and Connectors in one place — import and run them straight away.",
      },
      cliUi: {
        title: "CLI-launched Web UI",
        desc: "One command brings up the local App Server; open the browser to orchestrate and observe runs.",
      },
    },
    workflowTitle: "Command line → browser in three steps",
    workflowSubtitle: "Start, connect, operate — all on your machine.",
    workflow: {
      step1: {
        title: "Launch the runtime",
        desc: "Run the command in your terminal; the single executable starts the App Server locally.",
        cmd: "flowy-agent-store serve",
      },
      step2: {
        title: "Local App Server",
        desc: "The process serves a versioned protocol on localhost; the UI only talks to it through the SDK.",
        cmd: "http://localhost:8787",
      },
      step3: {
        title: "Open the Web UI",
        desc: "The browser opens the workbench; import an Agent and start a single or team run.",
        cmd: "Operate in the browser",
      },
    },
    downloadTitle: "Download Flowy Agent Store",
    downloadSubtitle: "Pick your platform, or see every build on the releases page.",
    download: {
      primaryCta: "Download for {{os}}",
      detectNote: "We detected your platform from your system",
      allPlatforms: "All platforms",
      manual: "Choose a platform",
      releaseNote: "See GitHub Releases for past versions and checksums",
      copy: "Copy command",
      copied: "Copied",
      fallbackCta: "Go to Releases",
    },
    platforms: {
      macos: "macOS",
      windows: "Windows",
      linux: "Linux",
      archAarch64: "Apple silicon",
      archX8664: "Intel / x64",
    },
  },
  docs: {
    title: "Docs",
    subtitle: "Quick start, CLI usage, architecture and compatibility.",
    onThisPage: "On this page",
    backToDocs: "Back to docs",
    notFound: "Document not found",
    notFoundBody: "Check the link or return to the docs home.",
    sections: {
      quickStart: "Quick start",
      cli: "CLI usage",
      architecture: "Architecture",
      compatibility: "Compatibility matrix",
    },
  },
  common: {
    notFound: "Page not found",
    home: "Back to home",
  },
};

export type Resources = typeof enUS;
export default enUS;
