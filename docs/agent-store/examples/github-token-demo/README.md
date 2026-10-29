# 示例市场：需要用户自备 token 的连接器条目（GitHub / 百度网盘）

这是一个**可以直接当市场用**的最小市场（市场名 `token-connectors`）：把它作为 `directory` 市场注册到宿主，
装出来的连接器就带一张密钥表单。格式规则见 `02-codebuddy-workbuddy-import-spec.md` §10.1；这里给的是那份
规范对应的**实物**，也是方案 `48-allo-connector-market-token-auth.zh.md` 落地的那棵树。

目录名还叫 `github-token-demo/`（历史原因），但市场身份是索引里的 `name`：`token-connectors`，含两个条目。

**条目 id 有意避开官方同名条目**：官方 `connectors` 市场（以及 WorkBuddy 的 283 条市场）里有 `github` 和
`baidu-netdisk`，显示名分别是 `GitHub` / `百度网盘`。本市场的两条因此叫 `github-token` /
`baidu-netdisk-token`，显示名带「自备 Token」后缀——否则宿主同时装两个市场时，商店里会出现两条同名
「百度网盘」，用户分不清哪个能填自己的 token。

## 目录结构（与 §10.1 的三处一一对应）

```
github-token-demo/
├── .codebuddy-connector/
│   └── connectors.json                    ① 市场索引：auth_mode = "token"（决定 credential.mode）
├── connectors/
│   ├── github-token/
│   │   ├── mcp.json                       ② 传输模板：Authorization 头里的 ${GITHUB_PAT} 是"落点"
│   │   ├── token-schema.json              ③ 表单：字段 + 双语文案 + 取密钥入口
│   │   └── skill/SKILL.md                 附属技能（可选；连接器组件来自 mcp.json）
│   └── baidu-netdisk-token/
│       ├── mcp.json                       ② 传输模板：SSE url 的 access_token 查询参数是"落点"
│       ├── token-schema.json              ③ 同上，docUrl 指向网盘开放平台鉴权文档
│       └── skill/SKILL.md
├── icons/
│   ├── github-token.svg                   条目头像：按**条目 id** 命名（icons/<entry-id>.svg）
│   ├── baidu-netdisk-token.svg            两个后缀同名文件是给官方同名条目留的兼容副本，见下
│   ├── github.svg
│   └── baidu-netdisk.svg
└── README.md
```

两个条目演示了引用落点的两种形态：**header**（GitHub）与 **URL 查询参数**（百度网盘 SSE，
口径来自[官方仓库](https://github.com/baidu-netdisk/mcp)的 SSE 用法）。`credential_mode()` 对两者都判
`token`——模板里出现 secret 引用即算声明，与落在 header 还是 url 无关。

头像走**约定路径**而不是字段：`connectors.json` 里没有 avatar 键（官方 228 条同样没有），
UI 按 `icons/<条目id>.svg` 拾取（`app_server_entry_assets.rs` 的 `market_icon_for`）；没有图标就退化成
名字首字母的彩色圆。这两个 svg 沿用官方市场 `market-source/connectors/icons/` 下的同一份资产
（同一服务的品牌图标，避免另制一份不一致的）。

**为什么每个条目有两个文件**：头像解析按条目 id 找 `icons/<id>.<ext>`，所以 `github-token.svg` 才是
真正被拾取的那份；`github.svg` / `baidu-netdisk.svg` 是官方同名条目用的名字，放在这里只是让这棵树在
被当成"官方市场的替代镜像"时也能命中——删掉不影响本市场自己的头像。

- ① 决定**有没有入口**：`auth_mode` 为 `server-side`／空时 `mode` 归 `none`，字段再多也不显示入口。
- ② 决定**值发去哪里**：漏了它，用户填完会看到「已配置」，而请求里什么也没有。
- ③ 决定**表单长什么样**：漏了它，`credential/set` 会以 `declares no credential form to fill` 拒绝。

## 怎么跑起来

三种方式，任选其一。**注意三处都会往宿主里写东西**（一条市场记录 + 一条连接器），用完按下面的"撤掉"清。

### 1. WebUI（最直接）

商店 → 右上角市场管理 → 添加市场 → 来源类型选**本地目录** → 路径填这个文件夹（到 `github-token-demo/` 这一层），
然后回商店搜索 `GitHub（自备` 或 `百度网盘（自备`，安装。

### 2. SDK / 协议面

```ts
const market = await client.addMarketplace({
  name: "token-connectors",
  source_kind: "directory",
  source: "<本仓库>/docs/agent-store/examples/github-token-demo",
});

const item = (await client.store.search("github-token")).find(
  (hit) => hit.marketplace_id === market.marketplace_id,
);
await client.store.install(item, { timeoutMs: 120_000 });

const connector = (await client.connectors.list()).find((row) => row.name === "github-token");
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

## 远程托管形态（zip + ModelScope，给所有 SDK 集成方）

上面三种方式都要本仓库检出路径。**给别人用**的形态是一个 zip 归档 + 公网地址，一行接入：

```ts
await client.addMarketplace({
  name: "token-connectors",
  source_kind: "zip",
  source: "https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master/token-connectors.zip",
});
// 然后 store/search("github-token") 或 store/search("baidu-netdisk-token") → store/install → 用户填 token → connector/call
```

**已发布**：上面那个地址现在真的能 `market/add`（真机装过两条）。归档的字节数与摘要不进本文档
（本文档自己就在归档里，写上去就等于每次发布都要改一次自己）——权威数字见
`docs/agent-store/48-allo-connector-market-token-auth.zh.md` §3.6。

挂在 ModelScope 而不是 GitHub Release：**国内网络访问 GitHub Release 的稳定性很差**，而官方市场
（`experts` / `skills` / `connectors`）本来就托管在同一个 ModelScope 模型库里，客户端对这条链路
（`X-Linked-Etag` = 内容 sha256）已有成熟回验。

上传（需要 `MODELSCOPE_API_TOKEN`，与官方市场同一条命令）：

```bash
python scripts/pack-connector-market.py                                   # → dist-market/token-connectors.zip
ms upload me9rez/flowy-marketplace dist-market/token-connectors.zip   --repo-type model --commit-message "Publish token-connectors market"
```

上传后必须回验远端摘要等于本地摘要（客户端靠它判断"没变就不重下"）：

```bash
curl -sI https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master/token-connectors.zip | grep -i x-linked-etag
python scripts/pack-connector-market.py        # 本地摘要以它的输出为准
```

其他事实：

- `zip` 源**只收 http(s) 地址**：本地路径是 `directory` 的活，由 source kind 决定而不是后缀
  （`nomifun-app/src/market_source.rs` 的 `normalize_zip_requires_http`）。
- 归档**根即市场根**（`fp-7` 起支持的形态）；用归档是为了避开 `url` 形态首次抓取逐文件镜像的代价。
- 重新打包：`python scripts/pack-connector-market.py`（确定性：固定时间戳与权限，同树两次构建
  字节相同，所以 sha256 能当版本标识）；`--check` 只校验确定性。
- 升级=重打包 + 覆盖上传；但注意**不可变快照**（`18` §11 D9）：改了条目内容必须连条目名
  或市场一起换，原地改会被拒。
- `store/install` 在**还没填 token** 时会等 `ready` 直到超时（两条条目都一样，实测约 3 分钟），
  返回 `ok: true` + `ready: false` / `readyIssue: "ready_timeout"`——这是正常的，装已经完成。
  集成方请传较小的 `timeoutMs`（如 20s）或忽略 `ready`。

方案与完整验收矩阵见 `docs/agent-store/48-allo-connector-market-token-auth.zh.md`。

## 撤掉

```ts
await client.uninstallInstall(snapshotId, componentIds);   // 先卸安装（组件 id 从 getInstallStatus 取）
await client.removeMarketplace(market.marketplace_id, true); // 再移除市场
```

WebUI 里对应「连接器 → 卸载」与「市场管理 → 移除」。

## 用本仓的门禁自查

```bash
bun scripts/check-agent-store-market.mjs --market token-connectors=docs/agent-store/examples/github-token-demo
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
- **名字已刻意错开**：条目 id 与显示名都和官方 `github` / `baidu-netdisk` 不同（见开头说明），所以同时装两个
  市场不会出现两条同名条目，也不会按名字 upsert 互相覆盖。官方 `github` 条目声明的是 `auth_mode: server-side`
  （平台侧持有凭据），所以它在界面里没有密钥入口。
