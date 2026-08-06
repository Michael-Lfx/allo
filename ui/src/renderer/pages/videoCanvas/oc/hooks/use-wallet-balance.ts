import { useCallback, useEffect, useRef, useState } from "react";

import { getWallet } from "@oc/services/api/wallet";

const WALLET_REFRESH_INTERVAL_MS = 30_000;

type WalletBalanceSnapshot = {
    userId: string;
    availableMicrocredits: number | null;
    refreshing: boolean;
};

export function useWalletBalance(userId?: string, enabled = true) {
    const activeUserId = enabled ? userId || "" : "";
    const requestSequence = useRef(0);
    const [snapshot, setSnapshot] = useState<WalletBalanceSnapshot>({ userId: "", availableMicrocredits: null, refreshing: false });

    const refresh = useCallback(async () => {
        const requestedUserId = activeUserId;
        const sequence = ++requestSequence.current;
        if (!requestedUserId) {
            setSnapshot({ userId: "", availableMicrocredits: null, refreshing: false });
            return;
        }
        setSnapshot((current) => ({
            userId: requestedUserId,
            availableMicrocredits: current.userId === requestedUserId ? current.availableMicrocredits : null,
            refreshing: true,
        }));
        try {
            const wallet = await getWallet(1, 1);
            if (sequence !== requestSequence.current) return;
            if (wallet.account.userId !== requestedUserId) throw new Error("积分账户与当前用户不一致");
            setSnapshot({ userId: requestedUserId, availableMicrocredits: wallet.account.availableMicrocredits, refreshing: false });
        } catch (error) {
            if (sequence !== requestSequence.current) return;
            console.warn("积分余额刷新失败", error);
            setSnapshot((current) => ({
                userId: requestedUserId,
                availableMicrocredits: current.userId === requestedUserId ? current.availableMicrocredits : null,
                refreshing: false,
            }));
        }
    }, [activeUserId]);

    useEffect(() => {
        if (!activeUserId) {
            requestSequence.current += 1;
            setSnapshot({ userId: "", availableMicrocredits: null, refreshing: false });
            return;
        }
        void refresh();
        const timer = window.setInterval(() => void refresh(), WALLET_REFRESH_INTERVAL_MS);
        const handleFocus = () => void refresh();
        const handleVisibility = () => {
            if (document.visibilityState === "visible") void refresh();
        };
        window.addEventListener("focus", handleFocus);
        window.addEventListener("wallet:updated", handleFocus);
        document.addEventListener("visibilitychange", handleVisibility);
        return () => {
            requestSequence.current += 1;
            window.clearInterval(timer);
            window.removeEventListener("focus", handleFocus);
            window.removeEventListener("wallet:updated", handleFocus);
            document.removeEventListener("visibilitychange", handleVisibility);
        };
    }, [activeUserId, refresh]);

    const snapshotMatchesUser = snapshot.userId === activeUserId;
    return {
        availableMicrocredits: snapshotMatchesUser ? snapshot.availableMicrocredits : null,
        refreshing: snapshotMatchesUser ? snapshot.refreshing : Boolean(activeUserId),
        refresh,
    };
}
