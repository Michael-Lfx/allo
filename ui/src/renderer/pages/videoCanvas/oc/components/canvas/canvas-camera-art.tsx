import { useId, type ReactElement } from "react";

import type { CameraApertureValue, CameraBodyId, CameraBodyOption, CameraLensId, CameraLensOption } from "@oc/lib/canvas/canvas-camera-rig";

const ART = "h-[58px] w-[80px]";

export function CameraBodyArt({ option }: { option: CameraBodyOption }) {
    const uid = useId().replace(/:/g, "");
    return (
        <svg viewBox="0 0 96 70" className={ART} aria-hidden="true">
            <ellipse cx="48" cy="62" rx="30" ry="3.6" fill="#000" opacity="0.28" />
            {BODY_ART[option.id](uid)}
        </svg>
    );
}

export function CameraLensArt({ option }: { option: CameraLensOption }) {
    const uid = useId().replace(/:/g, "");
    return (
        <svg viewBox="0 0 96 70" className={ART} aria-hidden="true">
            <ellipse cx="50" cy="62" rx="26" ry="3.4" fill="#000" opacity="0.26" />
            {LENS_ART[option.id](uid)}
        </svg>
    );
}

export function CameraFocalArt({ mm }: { mm: string }) {
    return (
        <svg viewBox="0 0 96 70" className={ART} aria-hidden="true">
            <text x="48" y="44" textAnchor="middle" fill="currentColor" fontSize={mm.length > 2 ? 28 : 34} fontWeight="650" letterSpacing="-1.4">
                {mm}
            </text>
        </svg>
    );
}

export function CameraApertureArt({ value }: { value: CameraApertureValue }) {
    const uid = useId().replace(/:/g, "");
    const inner = APERTURE_OPENING[value];
    const blades = 8;
    return (
        <svg viewBox="0 0 96 70" className={ART} aria-hidden="true">
            <defs>
                <radialGradient id={`${uid}-barrel`} cx="46%" cy="38%" r="70%">
                    <stop offset="0%" stopColor="#6a6e76" />
                    <stop offset="55%" stopColor="#2a2d33" />
                    <stop offset="100%" stopColor="#12141a" />
                </radialGradient>
                <linearGradient id={`${uid}-blade`} x1="0" y1="0" x2="1" y2="1">
                    <stop offset="0%" stopColor="#d8dde6" />
                    <stop offset="42%" stopColor="#8b919c" />
                    <stop offset="100%" stopColor="#3a3e46" />
                </linearGradient>
            </defs>
            <circle cx="48" cy="34" r="24" fill={`url(#${uid}-barrel)`} />
            <circle cx="48" cy="34" r="20.5" fill="#1a1c20" stroke="#9aa3ae" strokeWidth="1.2" />
            <circle cx="48" cy="34" r="18.2" fill="#0c0e12" />
            {Array.from({ length: blades }, (_, i) => (
                <path key={i} d={irisBlade(i, blades, 48, 34, inner, 17.4)} fill={`url(#${uid}-blade)`} stroke="#1c1e22" strokeWidth="0.45" />
            ))}
            <circle cx="48" cy="34" r={inner} fill="#07080b" />
            <circle cx="48" cy="34" r={Math.max(0.7, inner * 0.38)} fill="#e8eef6" opacity="0.22" />
            <path d="M38 24c6-5 14-5 20 1" fill="none" stroke="white" strokeWidth="1.1" opacity="0.28" />
        </svg>
    );
}

const APERTURE_OPENING: Record<CameraApertureValue, number> = {
    "1.4": 13.2,
    "1.8": 11.2,
    "2": 9.6,
    "2.8": 7.2,
    "4": 5.1,
    "5.6": 3.6,
    "8": 2.4,
    "11": 1.5,
};

function polar(cx: number, cy: number, r: number, a: number) {
    return { x: cx + Math.cos(a) * r, y: cy + Math.sin(a) * r };
}

function irisBlade(index: number, blades: number, cx: number, cy: number, inner: number, outer: number) {
    const step = (Math.PI * 2) / blades;
    const a0 = index * step - 0.18;
    const a1 = a0 + step * 1.42;
    const start = polar(cx, cy, outer, a0);
    const end = polar(cx, cy, outer, a1);
    const tip = polar(cx, cy, inner, a0 + step * 0.78);
    const bulge = polar(cx, cy, inner + (outer - inner) * 0.42, a0 + step * 0.46);
    return `M${start.x.toFixed(2)} ${start.y.toFixed(2)} A${outer} ${outer} 0 0 1 ${end.x.toFixed(2)} ${end.y.toFixed(2)} Q${bulge.x.toFixed(2)} ${bulge.y.toFixed(2)} ${tip.x.toFixed(2)} ${tip.y.toFixed(2)} Z`;
}

const BODY_ART: Record<CameraBodyId, (uid: string) => ReactElement> = {
    dxl2: (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-dxl`} x1="0" y1="0" x2="1" y2="1">
                    <stop offset="0%" stopColor="#5a5e66" />
                    <stop offset="100%" stopColor="#1a1d22" />
                </linearGradient>
            </defs>
            <rect x="22" y="16" width="4" height="12" rx="1" fill="#c4a574" />
            <rect x="58" y="16" width="4" height="12" rx="1" fill="#c4a574" />
            <rect x="22" y="14" width="40" height="5" rx="2" fill="#d8c09a" />
            <rect x="18" y="28" width="48" height="26" rx="4" fill={`url(#${uid}-dxl)`} />
            <rect x="22" y="32" width="18" height="10" rx="1.5" fill="#111318" />
            <rect x="8" y="31" width="14" height="12" rx="2" fill="#2a2e36" />
            <rect x="64" y="34" width="10" height="14" rx="2" fill="#2c3038" />
            <rect x="22" y="48" width="28" height="4" rx="1" fill="#c4a574" />
        </g>
    ),
    alexa35: (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-alx`} x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="#6a6e62" />
                    <stop offset="100%" stopColor="#1c1f18" />
                </linearGradient>
            </defs>
            <rect x="24" y="22" width="42" height="28" rx="5" fill={`url(#${uid}-alx)`} />
            <rect x="28" y="18" width="16" height="8" rx="2" fill="#2a2c26" />
            <circle cx="58" cy="28" r="3.2" fill="#e2b84a" />
            <rect x="14" y="30" width="12" height="10" rx="2" fill="#3a3c34" />
            <rect x="62" y="32" width="12" height="12" rx="2" fill="#252820" />
            <rect x="30" y="36" width="14" height="8" rx="1" fill="#12140f" />
        </g>
    ),
    venice2: (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-ven`} x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0%" stopColor="#4a5564" />
                    <stop offset="100%" stopColor="#151a22" />
                </linearGradient>
            </defs>
            <rect x="16" y="30" width="56" height="20" rx="3" fill={`url(#${uid}-ven)`} />
            <rect x="20" y="24" width="36" height="8" rx="1.5" fill="#8aa4c4" />
            <rect x="58" y="26" width="8" height="6" rx="1" fill="#5ad0ff" />
            <rect x="22" y="34" width="10" height="8" rx="1" fill="#0e1218" />
            <rect x="36" y="34" width="10" height="8" rx="1" fill="#0e1218" />
            <rect x="70" y="34" width="8" height="12" rx="1.5" fill="#2a3340" />
        </g>
    ),
    vraptor: (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-red`} x1="0" y1="0" x2="1" y2="1">
                    <stop offset="0%" stopColor="#c43a3a" />
                    <stop offset="100%" stopColor="#4a1010" />
                </linearGradient>
            </defs>
            <rect x="30" y="20" width="32" height="32" rx="4" fill={`url(#${uid}-red)`} />
            <rect x="40" y="14" width="12" height="8" rx="1.5" fill="#7a2020" />
            <rect x="36" y="28" width="12" height="8" rx="1" fill="#1a0808" />
            <circle cx="56" cy="30" r="2.4" fill="#ff6a6a" />
            <rect x="60" y="30" width="8" height="12" rx="1.5" fill="#3a1212" />
        </g>
    ),
    hasselblad: (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-has`} x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="#e8d2a8" />
                    <stop offset="45%" stopColor="#5a4634" />
                    <stop offset="100%" stopColor="#1b1612" />
                </linearGradient>
            </defs>
            <rect x="28" y="16" width="40" height="40" rx="7" fill={`url(#${uid}-has)`} />
            <rect x="40" y="12" width="16" height="8" rx="2" fill="#c4a878" />
            <circle cx="48" cy="38" r="11" fill="#111" stroke="#e0c48a" strokeWidth="1.6" />
            <circle cx="48" cy="38" r="4.5" fill="#ffe9c4" opacity="0.45" />
        </g>
    ),
    imax: (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-imax`} x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="#d8d8e0" />
                    <stop offset="100%" stopColor="#121218" />
                </linearGradient>
            </defs>
            <rect x="30" y="8" width="36" height="22" rx="10" fill={`url(#${uid}-imax)`} />
            <rect x="38" y="12" width="20" height="14" rx="6" fill="#2a2a32" />
            <rect x="22" y="30" width="52" height="24" rx="3" fill="#2a2a32" />
            <rect x="28" y="36" width="16" height="10" rx="1" fill="#0e0e14" />
            <rect x="70" y="36" width="8" height="12" rx="1" fill="#4a4a54" />
        </g>
    ),
};

const LENS_ART: Record<CameraLensId, (uid: string) => ReactElement> = {
    "signature-prime": (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-sig`} x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0%" stopColor="#7a8088" />
                    <stop offset="100%" stopColor="#14171c" />
                </linearGradient>
            </defs>
            <LensMount x={16} y={27} />
            <rect x="26" y="22" width="44" height="26" rx="4" fill={`url(#${uid}-sig)`} />
            <FocusGear x={32} y={24} width={28} height={22} />
            <rect x="30" y="24" width="22" height="3" rx="1" fill="#6ec4ff" opacity="0.9" />
            <LensGlass cx={74} cy={35} r={13} glass="#b9dfff" ring="#d9c07a" />
        </g>
    ),
    "cooke-s4": (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-cke`} x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0%" stopColor="#e8c48a" />
                    <stop offset="55%" stopColor="#8a5a28" />
                    <stop offset="100%" stopColor="#3a1e0c" />
                </linearGradient>
            </defs>
            <LensMount x={14} y={26} warm />
            <rect x="24" y="20" width="50" height="30" rx="5" fill={`url(#${uid}-cke)`} />
            <FocusGear x={30} y={23} width={32} height={24} />
            <rect x="28" y="32" width="30" height="6" rx="1" fill="#f3ddb4" opacity="0.55" />
            <LensGlass cx={78} cy={35} r={14} glass="#ffe1b0" ring="#e8c48a" />
        </g>
    ),
    "master-prime": (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-mp`} x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0%" stopColor="#4a5864" />
                    <stop offset="100%" stopColor="#10161c" />
                </linearGradient>
            </defs>
            <LensMount x={22} y={25} />
            <rect x="32" y="18" width="34" height="34" rx="6" fill={`url(#${uid}-mp)`} />
            <FocusGear x={36} y={22} width={18} height={26} />
            <LensGlass cx={70} cy={35} r={15} glass="#c8e8ff" ring="#9ab0c4" />
        </g>
    ),
    primo: (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-prm`} x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0%" stopColor="#6a6460" />
                    <stop offset="100%" stopColor="#161412" />
                </linearGradient>
            </defs>
            <LensMount x={8} y={28} />
            <rect x="18" y="23" width="56" height="24" rx="4" fill={`url(#${uid}-prm)`} />
            <FocusGear x={24} y={25} width={36} height={20} />
            <rect x="58" y="25" width="8" height="20" rx="1" fill="#c4a070" />
            <LensGlass cx={80} cy={35} r={12} glass="#dce8ff" ring="#c4a070" />
        </g>
    ),
    summilux: (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-lux`} x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0%" stopColor="#4a3a2a" />
                    <stop offset="100%" stopColor="#16120e" />
                </linearGradient>
            </defs>
            <rect x="24" y="26" width="12" height="18" rx="2" fill="#2a221c" />
            <rect x="34" y="22" width="28" height="26" rx="4" fill={`url(#${uid}-lux)`} />
            <circle cx="38" cy="28" r="2.2" fill="#d24a3a" />
            <rect x="40" y="40" width="16" height="3" rx="1" fill="#c4a070" opacity="0.75" />
            <LensGlass cx={66} cy={35} r={11} glass="#ffe8c8" ring="#e0b878" />
        </g>
    ),
    k35: (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-k35`} x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0%" stopColor="#d0d4dc" />
                    <stop offset="45%" stopColor="#5a5e66" />
                    <stop offset="100%" stopColor="#12151a" />
                </linearGradient>
            </defs>
            <LensMount x={16} y={27} />
            <rect x="26" y="20" width="40" height="30" rx="3" fill={`url(#${uid}-k35)`} />
            <rect x="34" y="24" width="18" height="5" rx="1" fill="#eef0f4" />
            <LensGlass cx={72} cy={35} r={13} glass="#c0d8ff" ring="#d0a050" />
        </g>
    ),
    anamorphic: (uid) => (
        <g>
            <defs>
                <linearGradient id={`${uid}-ana`} x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0%" stopColor="#5a4a62" />
                    <stop offset="100%" stopColor="#140e1a" />
                </linearGradient>
            </defs>
            <LensMount x={12} y={28} />
            <rect x="22" y="24" width="28" height="22" rx="3" fill={`url(#${uid}-ana)`} />
            <FocusGear x={26} y={26} width={18} height={18} />
            <ellipse cx="68" cy="35" rx="16" ry="21" fill="#1a1020" stroke="#e0b0ff" strokeWidth="1.7" />
            <ellipse cx="68" cy="35" rx="10" ry="14" fill="#2a1838" />
            <ellipse cx="63" cy="29" rx="5" ry="4" fill="#f0c8ff" opacity="0.38" />
        </g>
    ),
};

function LensMount({ x, y, warm = false }: { x: number; y: number; warm?: boolean }) {
    return <rect x={x} y={y} width="12" height="16" rx="2" fill={warm ? "#4a3018" : "#2a2e34"} stroke={warm ? "#c49a62" : "#5a616c"} strokeWidth="0.8" />;
}

function FocusGear({ x, y, width, height }: { x: number; y: number; width: number; height: number }) {
    const count = Math.max(6, Math.round(width / 3.2));
    return (
        <g>
            {Array.from({ length: count }, (_, i) => (
                <rect key={i} x={x + i * (width / count)} y={y} width="1.15" height={height} fill="#000" opacity="0.28" />
            ))}
        </g>
    );
}

function LensGlass({ cx, cy, r, glass, ring }: { cx: number; cy: number; r: number; glass: string; ring: string }) {
    return (
        <g>
            <circle cx={cx} cy={cy} r={r} fill="#0b0d12" stroke={ring} strokeWidth="1.6" />
            <circle cx={cx} cy={cy} r={r * 0.58} fill={glass} opacity="0.5" />
            <path d={`M${cx - 6} ${cy - 6}c4-3 10-3 13 1`} fill="none" stroke="white" strokeWidth="1.15" opacity="0.38" />
        </g>
    );
}
