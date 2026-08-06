/**
 * Wallet / credits API stub for allo canvas (credits disabled).
 */

export type CreditAccount = {
  userId: string;
  availableMicrocredits: number;
  reservedMicrocredits: number;
  version: number;
  createdAt: string;
  updatedAt: string;
};

export type CreditLedgerEntry = {
  id: string;
  userId: string;
  type: 'redeem' | 'admin_grant' | 'consume' | 'refund' | 'admin_adjustment' | 'signup_bonus' | 'checkin_bonus';
  amountMicrocredits: number;
  availableAfterMicrocredits: number;
  reservedAfterMicrocredits: number;
  billingOrderId?: string;
  model?: string;
  channelId?: string;
  scene?: string;
  note?: string;
  createdAt: string;
};

export type WalletSummary = {
  account: CreditAccount;
  entries: CreditLedgerEntry[];
  total: number;
  page: number;
  limit: number;
  policy: {
    signupBonusMicrocredits: number;
    checkinBonusMicrocredits: number;
    checkedInToday: boolean;
  };
};

export type CreditPolicy = {
  signupBonusMicrocredits: number;
  checkinBonusMicrocredits: number;
  defaultMultiplierBasisPoints: number;
  modelMultiplierBasisPoints: Record<string, number>;
};

export type ChannelModel = {
  id: string;
  channelId: string;
  modelKey: string;
  displayName: string;
  capability: 'text' | 'image' | 'video' | 'audio' | '';
  protocol?: import('@oc/lib/model-protocols').ModelProtocol;
  billingMode: 'fixed_request' | 'per_second' | 'token';
  unitPriceMicrocredits: number;
  inputTokenPriceMicrocredits: number;
  outputTokenPriceMicrocredits: number;
  cachedTokenPriceMicrocredits: number;
  priceConfigured: boolean;
  enabled: boolean;
  priceVersion: number;
  capabilityVersion?: number;
  capabilityConfig?: import('@oc/lib/model-capabilities').ModelCapabilityConfig;
  createdAt: string;
  updatedAt: string;
};

export type LinuxDOSetting = {
  enabled: boolean;
  clientId: string;
  clientSecret?: string;
  hasClientSecret: boolean;
  authorizationUrl: string;
  tokenUrl: string;
  userInfoUrl: string;
  redirectUrl: string;
  scopes: string[];
  clientAuthMethod: 'client_secret_post' | 'client_secret_basic';
  subjectField: string;
  usernameField: string;
  displayNameField: string;
  emailField: string;
  avatarField: string;
  updatedAt?: string;
};

export type RegistrationSetting = {
  enabled: boolean;
  updatedBy?: string;
  createdAt?: string;
  updatedAt?: string;
};

export type EmailSetting = {
  enabled: boolean;
  host: string;
  port: number;
  username: string;
  password?: string;
  encryption: 'starttls' | 'tls' | 'none';
  fromEmail: string;
  fromName: string;
  hasPassword: boolean;
  updatedBy?: string;
  createdAt?: string;
  updatedAt?: string;
};

export type RedeemBatch = {
  id: string;
  amountMicrocredits: number;
  count: number;
  note?: string;
  createdBy: string;
  expiresAt?: string;
  createdAt: string;
  availableCount: number;
  redeemedCount: number;
  disabledCount: number;
  expiredCount: number;
};

export type AdminRedeemCode = {
  id: string;
  code?: string;
  codeSuffix: string;
  status: 'unused' | 'redeemed' | 'disabled' | 'expired';
  redeemedBy?: string;
  redeemedUsername?: string;
  redeemedDisplayName?: string;
  redeemedAt?: string;
  redeemedIp?: string;
  expiresAt?: string;
  amountMicrocredits: number;
};

export type AdminRedeemCodePage = {
  batch: RedeemBatch;
  codes: AdminRedeemCode[];
  plaintextAvailable: boolean;
  total: number;
  page: number;
  limit: number;
};

export type BillingOrder = {
  id: string;
  userId: string;
  taskId?: string;
  channelId: string;
  model: string;
  capability: string;
  scene: string;
  billingMode: 'fixed_request' | 'per_second' | 'token';
  unitPriceMicrocredits: number;
  multiplierBasisPoints: number;
  quantity: number;
  amountMicrocredits: number;
  reservedAmountMicrocredits: number;
  actualAmountMicrocredits: number;
  refundedAmountMicrocredits: number;
  inputTokenPriceMicrocredits: number;
  outputTokenPriceMicrocredits: number;
  cachedTokenPriceMicrocredits: number;
  inputTokens: number;
  outputTokens: number;
  cachedTokens: number;
  usageAvailable: boolean;
  status: 'reserved' | 'running' | 'settled' | 'refunded' | 'uncertain';
  providerRequestId?: string;
  error?: string;
  resolvedBy?: string;
  resolutionNote?: string;
  createdAt: string;
  updatedAt: string;
};

export type AdminFinanceListParams = {
  keyword?: string;
  status?: string;
  validity?: string;
  page?: number;
  limit?: number;
};

const zeroAccount = (): CreditAccount => {
  const now = new Date().toISOString();
  return {
    userId: 'local',
    availableMicrocredits: 0,
    reservedMicrocredits: 0,
    version: 0,
    createdAt: now,
    updatedAt: now,
  };
};

const adminUnavailable = () => Promise.reject(new Error('Admin wallet APIs are not available in allo canvas'));

export function getWallet(page = 1, limit = 30, _type = 'all') {
  return Promise.resolve({
    account: zeroAccount(),
    entries: [] as CreditLedgerEntry[],
    total: 0,
    page,
    limit,
    policy: {
      signupBonusMicrocredits: 0,
      checkinBonusMicrocredits: 0,
      checkedInToday: false,
    },
  } as WalletSummary);
}

export function redeemCredits(_code: string) {
  return Promise.reject(new Error('Credits are disabled in allo canvas'));
}

export function checkinCredits() {
  return Promise.resolve({ account: zeroAccount(), granted: false });
}

export function getAdminCreditPolicy() {
  return adminUnavailable() as Promise<{ policy: CreditPolicy }>;
}

export function updateAdminCreditPolicy(_policy: CreditPolicy) {
  return adminUnavailable() as Promise<{ policy: CreditPolicy }>;
}

export function getAdminLinuxDOSetting() {
  return adminUnavailable() as Promise<{ setting: LinuxDOSetting }>;
}

export function updateAdminLinuxDOSetting(_input: Partial<LinuxDOSetting>) {
  return adminUnavailable() as Promise<{ setting: LinuxDOSetting }>;
}

export function getAdminRegistrationSetting() {
  return adminUnavailable() as Promise<{ setting: RegistrationSetting }>;
}

export function updateAdminRegistrationSetting(_enabled: boolean) {
  return adminUnavailable() as Promise<{ setting: RegistrationSetting }>;
}

export function getAdminEmailSetting() {
  return adminUnavailable() as Promise<{ setting: EmailSetting }>;
}

export function updateAdminEmailSetting(_input: Partial<EmailSetting>) {
  return adminUnavailable() as Promise<{ setting: EmailSetting }>;
}

export function listAdminChannelModels(_channelId: string) {
  return Promise.resolve({ models: [] as ChannelModel[] });
}

export function fetchAdminChannelModels(_channelId: string) {
  return Promise.resolve({ models: [] as string[], added: 0 });
}

export function testAdminChannelModel(
  _channelId: string,
  _input: Pick<ChannelModel, 'modelKey' | 'capability' | 'protocol'> & {
    capabilityConfig?: ChannelModel['capabilityConfig'];
  }
) {
  return adminUnavailable() as Promise<{ durationMs: number }>;
}

export function createAdminChannelModel(
  _channelId: string,
  _input: Omit<ChannelModel, 'id' | 'channelId' | 'priceVersion' | 'createdAt' | 'updatedAt'>
) {
  return adminUnavailable() as Promise<{ model: ChannelModel }>;
}

export function updateAdminChannelModel(
  _channelId: string,
  _id: string,
  _input: Omit<ChannelModel, 'id' | 'channelId' | 'priceVersion' | 'createdAt' | 'updatedAt'>
) {
  return adminUnavailable() as Promise<{ model: ChannelModel }>;
}

export function deleteAdminChannelModel(_channelId: string, _id: string) {
  return Promise.resolve({ ok: true });
}

export function listAdminRedeemBatches(_params: AdminFinanceListParams = {}) {
  return Promise.resolve({ batches: [] as RedeemBatch[], total: 0, page: 1, limit: 30 });
}

export function createAdminRedeemBatch(_input: {
  amountMicrocredits: number;
  count: number;
  note?: string;
  expiresAt?: string;
}) {
  return adminUnavailable() as Promise<{ batch: RedeemBatch; codes: string[] }>;
}

export function listAdminRedeemBatchCodes(
  _batchId: string,
  _params: { status?: string; page?: number; limit?: number } = {}
) {
  return adminUnavailable() as Promise<AdminRedeemCodePage>;
}

export function disableAdminRedeemBatch(_batchId: string) {
  return Promise.resolve({ disabledCount: 0 });
}

export function disableAdminRedeemCode(_batchId: string, _codeId: string) {
  return Promise.resolve({ ok: true });
}

export function adjustAdminUserCredits(
  _userId: string,
  _input: { amountMicrocredits: number; note: string }
) {
  return adminUnavailable() as Promise<{ account: CreditAccount }>;
}

export function listAdminBillingOrders(_params: AdminFinanceListParams = {}) {
  return Promise.resolve({ orders: [] as BillingOrder[], total: 0, page: 1, limit: 30 });
}

export function resolveAdminBillingOrder(
  _id: string,
  _input: { action: 'settle' | 'refund'; note: string }
) {
  return adminUnavailable() as Promise<{ order: BillingOrder }>;
}

export function resolveAdminBillingOrders(_input: {
  ids: string[];
  action: 'settle' | 'refund';
  note: string;
}) {
  return Promise.resolve({ resolvedCount: 0, failed: [] as Array<{ id: string; message: string }> });
}
