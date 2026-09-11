const enUS = {
  nav: {
    market: "Market",
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
    resources: "Resources",
    releases: "Releases",
    github: "GitHub",
    copyright:
      "© Flowy Agent Store contributors. This site and runtime are local-first by design.",
  },
  landing: {
    eyebrow: "Local-first · Single-file runtime",
    heroTitle: "Your local-first agent workbench",
    heroSubtitle:
      "A single executable that opens into a full workbench in your browser. Import experts, form teams, orchestrate runs — execution, credentials and run state never leave your machine; the cloud only syncs definitions and versions.",
    heroCtaDownload: "Download",
    heroCtaMarket: "Browse the market",
    heroCtaDocs: "Read the docs",
    heroTerminalListening: "App Server is live at http://127.0.0.1:8787",
    heroTerminalOpened: "Workbench opened in your browser",
    showcase: {
      label: "Product preview",
      runTitle: "Team Run · frontend-backend-experts",
      status: "Running",
      navCatalog: "Catalog",
      navRuns: "Runs",
      navArtifacts: "Artifacts",
      tlPlan: "Plan ready · 4-step DAG",
      tlTool: "Tool call · browser.open",
      tlArt: "Artifact · todo-api.ts",
    },
    featureTitle: "Built for the local workbench",
    featureSubtitle: "Import, run, observe — all inside one trusted local process.",
    features: {
      workbench: {
        title: "Local agent catalog",
        desc: "Import and search experts, skills and connectors in one place; immutable snapshots you can query and run right away.",
      },
      observability: {
        title: "Full run observability",
        desc: "Plan (DAG), event timeline and artifacts stream in live; both single-agent and team runs are replayable.",
      },
      localFirst: {
        title: "Local-first execution",
        desc: "Credentials and run state stay on your machine; the cloud only syncs the market catalog, versions and distribution.",
      },
      oneCmd: {
        title: "One command to start",
        desc: "No databases, no resident services; flowy-agent-store boots and the browser opens ready to work.",
      },
    },
    workflowTitle: "Command line → workbench in three steps",
    workflowSubtitle: "Download, import, run — all on your machine.",
    workflow: {
      step1: {
        title: "Launch the runtime",
        desc: "Run the command; the single executable starts the local App Server and opens the workbench.",
        cmd: "flowy-agent-store",
      },
      step2: {
        title: "Import experts",
        desc: "Import CodeBuddy / WorkBuddy plugins from the workbench, or call installStoreEntry() via the SDK.",
        cmd: "Import CodeBuddy / WorkBuddy plugins",
      },
      step3: {
        title: "Run & observe",
        desc: "Pick an agent or a team run and watch the DAG, event timeline and artifacts in real time.",
        cmd: "Run → DAG · Timeline · Artifacts",
      },
    },
    marketStrip: {
      title: "Market resources, ready out of the box",
      subtitle: "Experts, skills and connectors import into your local catalog in one click, continuously updated.",
      cta: "Open the market",
    },
    dev: {
      title: "Built for developers",
      subtitle:
        "A type-safe TypeScript SDK: types only, connect to a running App Server, or launch the whole runtime in one call.",
      pkgProtocol: {
        name: "@flowy-agent-store/protocol",
        desc: "The single source of wire-protocol types: requests, responses, notifications and errors. Zero runtime.",
      },
      pkgClient: {
        name: "@flowy-agent-store/client",
        desc: "AppServerClient and 7 sub-clients with a Transport abstraction for WS / HTTP.",
      },
      pkgSdk: {
        name: "@flowy-agent-store/sdk",
        desc: "Spawns the binary → loopback WebSocket → readiness handshake, returning a ready client.",
      },
      codeTitle: "quick-start.ts",
      cta: "Read the TypeScript SDK guide",
    },
    faq: {
      title: "FAQ",
      items: {
        q1: {
          q: "What exactly stays local?",
          a: "Execution, credentials and run state live only on your machine; the cloud is used to sync the market catalog, versions and distribution. No cloud execution, no run data uploaded.",
        },
        q2: {
          q: "Which platforms are supported?",
          a: "Only Windows x64 builds are published today; macOS (Apple silicon / Intel) and Linux (x64 / arm64) are not available yet and need a separate decision before they open up.",
        },
        q3: {
          q: "Do I need an account?",
          a: "Local runs need no login (trusted local mode). When exposing the server to your LAN (--host 0.0.0.0), enable --auth; the admin account is created on first start.",
        },
        q4: {
          q: "What can I import?",
          a: "CodeBuddy plugins and WorkBuddy skills / connectors become immutable snapshots through the importer; once imported they are queryable and runnable in the catalog. Resources with unconfirmed licenses never enter public distribution.",
        },
      },
    },
    stats: {
      s1: { value: "1", label: "Single executable" },
      s2: { value: "0", label: "Databases or services to install" },
      s3: { value: "4", label: "Catalog kinds managed" },
      s4: { value: "2", label: "UI and doc languages" },
    },
    downloadTitle: "Download Flowy Agent Store",
    downloadSubtitle: "Pick your platform, or browse every build on the downloads page.",
    download: {
      primaryCta: "Download for {{os}}",
      detectNote: "Platform detected from your current system",
      allPlatforms: "All platforms",
      psTitle: "One-line PowerShell install (npm)",
      psHint:
        "Paste this line into PowerShell: installs the npm runtime package and adds agent-store to your user PATH (Node.js / npm required, no admin rights).",
      psView: "View the install script source",
      manual: "Choose a platform manually",
      releaseNote: "See the download center for older versions and checksums",
      copy: "Copy command",
      copied: "Copied",
      fallbackCta: "Go to releases",
      unavailableNote: "Only Windows x64 is published; other platforms are not available yet.",
    },
    platforms: {
      macos: "macOS",
      windows: "Windows",
      linux: "Linux",
      archAarch64: "Apple silicon",
      archX8664: "Intel / x64",
    },
    marketStat: "resources in the market",
  },
  market: {
    title: "Resource Market",
    subtitle: "Experts, skills and connectors — browse and search in one place.",
    updated: "Data updated {{date}}.",
    searchPlaceholder: "Search names, descriptions or tags…",
    tabs: {
      experts: "Experts",
      skills: "Skills",
      connectors: "Connectors",
    },
    empty: "No matching resources.",
    emptyHint: "Try another keyword, or switch tabs.",
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
      pluginsMarket: "Plugins & Market",
      typescriptSdk: "TypeScript SDK",
      upgrade: "Upgrade & migration",
      changelog: "Changelog",
      configuration: "Configuration",
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