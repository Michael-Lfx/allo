# 示例：一个需要用户填 key 的连接器条目

这是一个**可以直接当市场用**的最小示例：把它作为 `directory` 市场注册到宿主，装出来的连接器就带一张
密钥表单。格式规则见 `02-codebuddy-workbuddy-import-spec.md` §10.1；这里给的是那份规范对应的**实物**。

## 目录结构（与 §10.1 的三处一一对应）

```
github-token-demo/
├── .codebuddy-connector/
│   └── connectors.json                    ① 市场索引：auth_mode = "token"（决定 credential.mode）
├── connectors/
│   └── github-token-demo/
│       ├── mcp.json                       ② 传输模板：headers 里的 ${GITHUB_PAT} 是"落点"
│       ├── token-schema.json              ③ 表单：字段 + 双语文案 + 取密钥入口
│       └── skill/SKILL.md                 附属技能（可选；连接器组件来自 mcp.json）
└── README.md
```

- ① 决定**有没有入口**：`auth_mode` 为 `server-side`／空时 `mode` 归 `none`，字段再多也不显示入口。
- ② 决定**值发去哪里**：漏了它，用户填完会看到「已配置」，而请求里什么也没有。
- ③ 决定**表单长什么样**：漏了它，`credential/set` 会以 `declares no credential form to fill` 拒绝。

## 怎么跑起来

三种方式，任选其一。**注意三处都会往宿主里写东西**（一条市场记录 + 一条连接器），用完按下面的"撤掉"清。

### 1. WebUI（最直接）

商店 → 右上角市场管理 → 添加市场 → 来源类型选**本地目录** → 路径填这个文件夹（到 `github-token-demo/` 这一层），
然后回商店搜索 `GitHub（示例`，安装。

### 2. SDK / 协议面

```ts
const market = await client.addMarketplace({
  name: "github-token-demo",
  source_kind: "directory",
  source: "<本仓库>/docs/agent-store/examples/github-token-demo",
});

const item = (await client.store.search("github-token-demo")).find(
  (hit) => hit.marketplace_id === market.marketplace_id,
);
await client.store.install(item, { timeoutMs: 120_000 });

const connector = (await client.connectors.list()).find((row) => row.name === "github-token-demo");
console.log(JSON.stringify(connector.credential, null, 2));
// → mode: "token"、status: "requires_input"、missing: ["GITHUB_PAT"]、fields[0].kind: "secret"
```

### 3. 命令行（只想看条目能不能被解析）

把它作为市场加进去即可，`store/search` 能搜到就说明索引与条目结构没问题。

## 装完之后应该看到什么

| 阶段 | `connector/test` | 说明 |
|---|---|---|
| 没填 key | `MCP_MISSING_CREDENTIAL`（`Missing credential reference: GITHUB_PAT`） | **fail-closed**：一个请求都不会发出去 |
| 填了 key | 走通；key 不合法则是 `MCP_HTTP_ERROR`（端点回 401/400） | 引用被替换成真值后发出 |

想**确认值真的发出去了**又不想用真 token，把 `mcp.json` 的 `url` 换成一台本地 mock server
（例如 `http://127.0.0.1:PORT/mcp`），让它记录收到的 `Authorization` 头——空的时候一个请求都收不到，
填完之后收到的就是 `Bearer <你填的值>`。参考实现见 `web/scripts/verify-connector-credentials-live.ts`。

## 撤掉

```ts
await client.uninstallInstall(snapshotId, componentIds);   // 先卸安装（组件 id 从 getInstallStatus 取）
await client.removeMarketplace(market.marketplace_id, true); // 再移除市场
```

WebUI 里对应「连接器 → 卸载」与「市场管理 → 移除」。

## 用本仓的门禁自查

```bash
bun scripts/check-agent-store-market.mjs --market github-token-demo=docs/agent-store/examples/github-token-demo
```

会输出 `✓ … (connector-market)`、`0 error(s)`，外加一条 **info**：`_files.txt# listing.absent`。
那条**对本地目录市场是预期的**——`_files.txt`（`18` §6）是**远程镜像**用来列出要同步哪些文件的清单，
本地 `directory` 源直接从盘上读条目，不需要它；没有它只意味着"条目标记为 external（不镜像内容）"，
不影响目录源的导入与安装。

## 几条容易踩的

- **这是第三方 `directory` 市场**：`market/add` 按源地址判定官方/第三方，本示例是第三方，`auto_update` 为 false。
- **别改这条目的内容而不换版本**：connector 条目的版本由条目自带的 manifest 决定，而导入请求不带索引里的
  `version`——改了内容、抬了索引版本仍会被"不可变快照"拒绝（实测见 `18` §11 D9）。示例阶段改内容请连条目名一起换。
- **`defaultValue` 不要写**：`secret` 字段的默认值会在导入期被丢弃并告警；真实市场里 `cisp-mcp` 那份就带了一把真 key，
  属于反面例子。
- **名字冲突**：若你已从官方市场装过同名连接器，装本示例会按名字 upsert 覆盖那一行；先卸载再装更干净。
- 这里的 `github-token-demo` 是**示例**，不是官方条目；官方 `github` 条目声明的是 `auth_mode: server-side`
  （平台侧持有凭据），所以它在界面里没有密钥入口。
