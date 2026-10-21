-- 概念铸名出处列（smoke-hardening t8，t2 条款 3 补完）。
--
-- learning_concept_registry.source：铸名的 attestation 出处说明（「教练提
-- 名：……」——谁在哪个语境铸的名），可空、纯簿记不进门禁（learnhub 口径
-- 「出处是簿记不是判定」）。**不做书目引用**：allo 无教材数据源，模型写
-- 「人教版七年级上册」必是编造（never-invent）；learnhub 有真实教材数据
-- 才写书引用。旧行缺列读侧默认 NULL。
--
-- v3 contract: 无物理外键、无触发器。append-only（ALTER ADD COLUMN）。

ALTER TABLE learning_concept_registry ADD COLUMN source TEXT;
