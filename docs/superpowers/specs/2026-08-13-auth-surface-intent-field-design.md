# Flowy Auth Surface and Kinetic Blueprint

Status: implemented on `feat/login-page-optimization`

## Scope

This specification covers `/cloud-login`, local `/login`, and the developer
settings cloud-login flow. It changes the shared frontend Auth Surface,
visual composition, motion, accessibility, and theme mapping only.
Authentication HTTP/IPC contracts, persistence, routing, model
synchronization, analytics semantics, and the six-digit OTP behavior remain
unchanged.

## Design direction

The login surface is a calm working surface at rest. The left side presents a
two-dimensional Kinetic Blueprint: a hand-placed editorial composition of
ten primary work fragments, five quiet companion workstations, registration
marks, seven short route segments, three static support lines, and a compact
review-summary document. It describes Intent → Work → Result without
pretending to be a map, network topology, dashboard, or ordinary flowchart.

The right side remains the primary task surface. The left visual is decorative,
semantic states are still communicated by the form and status bar, and no
authentication information is available only through animation.

## Layout contract

| Surface | Wide | Narrow | Business behavior |
| --- | --- | --- | --- |
| Cloud login | 56/44 AuthShell, optional desktop controls | blueprint brand band then form | auto-starts email session |
| Local login | same AuthShell, no desktop controls | blueprint brand band then form | keeps local login/setup behavior |
| Settings cloud login | existing settings primitives | row content stacks naturally | explicit Start action |

The shared AuthShell keeps its 1040px maximum width and 560px desktop minimum
height. At 899px the left side becomes a compact brand band with four primary
fragments and no companion system. At 639px it uses the small static
path-result signature so the form and keyboard remain primary.
Settings does not embed the brand field.

## Kinetic Blueprint contract

`IntentField` retains the existing `mode`, `phase`, `activationLevel`, and
`inputEnergy` props and accepts the visual-only `blueprintStep` prop.
Geometry lives in `blueprintScene.ts` as a deterministic hand-placed scene:

- Ten primary fragments cover command, file, terminal, browser, diff, table,
  summary, and review shapes. Five companion fragments add terminal, diff,
  file, table, and summary workstations without entering the primary route.
- Seven short route segments provide the main visual rhythm. Their restrained
  diagonal turns and uneven lengths keep the composition editorial rather
  than diagrammatic. Each segment owns the fragments it delivers, so route
  order, document arrival, and backplate occlusion cannot drift apart.
- Arrived documents become opaque and mask the route below them. Document
  interiors contain only horizontal rules, dots, or check marks; page
  fold corners remain part of the outer silhouette.
- Documents are hand-placed on alternating rails around the route with clear
  negative space, varied scale, and restrained rotation. Their bounding boxes
  do not overlap; only the intentional review and final summary connections
  pass beneath a document.
- The former empty result frame is removed. A small `review-summary` document
  sits at the final route endpoint with summary lines and a success check.
- Registration marks, short dimensions, and crop lines provide precision
  without introducing explanatory micro-copy.
- The SVG uses a stable `viewBox`, remains `aria-hidden`, and never enters the
  keyboard order or accessibility tree.
- Companion documents remain quiet background stations. Three static cool-gray
  support lines follow the approved document-edge anchors: an upper-left line
  between the terminal stations, an upper-right short line between the file and
  table documents, and a lower open right-angle line between the file and
  summary documents. They are rendered behind
  opaque document backplates and never receive accent, focus, error, or completion
  state. At 640–899px both the support lines and all companion documents disappear as
  one semantic responsive group; below 640px the whole companion system stays
  hidden.
- Every document may carry deterministic `BlueprintDetail` data. Detail lines,
  edge dots, and review checks use `revealAt` steps so the
  checks occupy reserved blank rails or cells instead of covering a document rule,
  and the
  OTP sequence enriches existing documents instead of mounting a new card
  stack. `BlueprintAmbientMark` adds a small set of locators and a completion
  anchor; these marks never participate in auth
  state calculation.

The scene has no Canvas, WebGL, Three.js, runtime particle generator, random
coordinates, gradient, blur, glow, or 3D transform.

## State and motion contract

| Auth state | Blueprint response |
| --- | --- |
| idle | quiet full composition; path mostly dormant |
| input | email typing remains quiet; local fields use coarse thresholds |
| code-sent | the second route segment is ready and the OTP checkpoint queue is available |
| verifying | the final route segment completes and one solid cursor travels it once |
| success | all fragments settle into the review-summary result document and one short pulse plays |
| warning | current progress remains visible with reduced result emphasis |
| error | one local fragment receives a danger treatment; layout does not shake |

Cloud OTP digit progress maps to four checkpoints at digits 1, 3, 5, and 6.
The visual step protocol is 0–7: idle, email submit, OTP screen, the four
checkpoints, and verification. Pasted or autofilled codes use a bounded queue
of at most four OTP checkpoint advances. Local username and password fields
map to the same scene through coarse `inputEnergy` thresholds, so both entry
points share one visual language without per-character route movement. Cloud
email typing is the one intentional input preview: `inputEnergy` draws only a
short portion of the first route segment and lifts its first document from
quiet to active; it never translates or remounts the whole scene.

Motion uses 120ms form feedback, 360ms route/document transitions, and 480ms
cursor/completion transitions with the existing enter/move easings. Route
stroke progress, fragment opacity, and the opaque backplate interpolate over
360ms, while the finite route queue advances every 400ms so line arrival and
document reveal remain visibly continuous rather than discrete jumps.
Input changes use transitions only; they never remount or translate the whole
scene. State changes use finite SVG transitions and one-shot keyframes. The
verifying cursor has no trail or loop. After cloud success, the form is
replaced by a stable loading state while the session refreshes and the `/guid`
route chunk is prefetched. The refresh returns an explicit outcome; an offline
or server failure becomes a recoverable preparation state with a retry action,
not an indefinite loading screen. One replace navigation then enters the home
page without rendering the signed-in account form or route fallback in between.
Navigation and authentication callbacks are never delayed for the visual
transition. Invalid verification uses a stable
`CLOUD_OTP_INVALID_CODE` / HTTP 422 contract; the pending session is retained
so the client can clear the six digits, focus the first cell, and retry without
resending. Expiry and attempt limits remain session-expired states, while
transport errors retain the code. A JSON `status: "failed"` response is a
terminal session failure rather than an invalid-code response; the client clears
the consumed session and exposes resend recovery without showing the backend
error text.

Fine-pointer feedback is enabled only for `(min-width: 900px) and (hover:
hover) and (pointer: fine)` and when Reduced Motion is off. A coalesced pointer
update can focus the nearest two fragments, moving each by at most 3px and
revealing one crosshair.
Pointer leave cancels the scheduled frame. There is no pointer trail, press
pulse, ambient shimmer, or retained animation loop.

Visibility changes clear pointer state and stop visual transitions. Reduced
Motion, coarse pointers, touch input, hidden pages, and unmount leave no
pending animation frame and show the current semantic state directly.

## Theme contract

The scene derives local variables from the existing semantic theme system:

- `--auth-blueprint-canvas`
- `--auth-blueprint-ink`
- `--auth-blueprint-muted`
- `--auth-blueprint-hairline`
- `--auth-blueprint-surface`
- `--auth-blueprint-backplate`
- `--auth-blueprint-surface-quiet`
- `--auth-blueprint-line-quiet`
- `--auth-blueprint-line-document`
- `--auth-blueprint-accent`
- `--auth-blueprint-accent-soft`
- `--auth-blueprint-danger`
- `--auth-blueprint-motion-route`
- `--auth-blueprint-motion-document`
- `--auth-blueprint-motion-cursor`
- `--auth-blueprint-motion-complete`

Light mode reads as an inked paper field with raised paper documents, so white
documents remain distinct from the canvas. Dark mode
uses raised blue-gray document planes and cool gray construction lines rather
than a simple inversion. Geometry and state meaning stay identical across
themes; only material, opacity, and contrast change. Decorative lines remain
quiet while meaningful path, check, and failure marks maintain non-text
contrast. Arrived document backplates and the review-summary document are
opaque in both themes so the route disappears beneath them. No empty rectangle
is placed against the form-panel split.

## OTP state contract

Email OTP remains fixed at six numeric characters. `OtpCodeInput` keeps one
real text input with numeric input mode, one-time-code autocomplete, and six
decorative cells grouped 3 + 3. Paste, autofill, Backspace, arrow keys, Enter,
automatic verification, and the 60-second cooldown remain unchanged.

The shared controller owns the email, code, pending session, request
generation, failure classification, cooldown, localized message, and recovery
action. The backend exposes `CLOUD_OTP_INVALID_CODE` with HTTP 422 for an
explicit invalid code. The frontend gives that code priority and keeps legacy
400/422 and structured Chinese/English message compatibility without rendering
backend text. Invalid-code and unknown failures preserve the session for retyping;
explicit expiry consumes the session and exposes resend recovery; transport
failures retain the code and allow retry verification. When an invalid or
unknown failure clears a complete code, `OtpCodeInput` refocuses the first
cell without scrolling so the next code can be typed immediately. Session
expiry remains disabled and does not steal focus. Backend text is never
rendered directly.

## Verification matrix

Automated proof covers:

- SVG blueprint structure and deterministic geometry bounds.
- Cloud and local state-to-progress mapping.
- Six-cell OTP accessibility and shared controller wiring.
- Reduced Motion and visibility cleanup markers.
- Local blueprint theme tokens and responsive simplification.
- Route-step ordering, document occlusion, four OTP checkpoints, and one-shot
  verifying cursor.
- Short localized error copy, complete-code reset focus, and transport/session
  recovery distinctions.
- Ten primary documents, five companion documents, three static support lines,
  deterministic `revealAt` details, and ambient marks;
  companion workstations never enter primary activation or pointer focus.
- Absence of Canvas, WebGL, Three.js, random generation, and ambient timers.

Run the focused Auth Surface test, TypeScript typecheck, project check, and UI
build. Manual acceptance remains required in both light and dark themes at
1440×900, 1280×800, 900px, 768px, and 360px, including keyboard focus, 200%
zoom, touch input, fine-pointer hover, reduced motion, OTP paste/autofill,
cooldown, recovery states, and desktop window controls.

The earlier split-layout specification remains historical. This document is
the current Auth Surface contract.
