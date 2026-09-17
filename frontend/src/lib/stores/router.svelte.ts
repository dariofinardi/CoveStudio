// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Minimal SPA router (plan §13). No history integration yet — the app
 * is a desktop shell, not a website. Pre-unlock routes (boot/setup/
 * unlock) precede the authenticated feature routes.
 */
import { untrack } from 'svelte'

export type Route =
  | 'boot'
  | 'setup'
  | 'unlock'
  | 'assistant'
  | 'projects'
  | 'tabular'
  | 'workflows'
  | 'templates'
  | 'settings'

/** Feature routes — the ones rendered inside the authenticated Shell. */
export const FEATURE_ROUTES = [
  'assistant',
  'projects',
  'tabular',
  'workflows',
  'templates',
  'settings',
] as const
export type FeatureRoute = (typeof FEATURE_ROUTES)[number]

export function isFeatureRoute(route: Route): route is FeatureRoute {
  return (FEATURE_ROUTES as readonly string[]).includes(route)
}

/**
 * Per-route "drill-down" data that a destination screen can read on
 * mount to restore a nested view — e.g. the Projects screen opening a
 * specific project's detail page when the user navigates back from a
 * chat that was launched from inside that project. Free-form because
 * each route owns its own restoration shape; today the only field is
 * `projectId` but new screens can add fields without churn.
 */
export type NavContext = {
  projectId?: string
}

/**
 * One "back" target on the navigation stack. `label` is what the
 * destination screen renders next to its back arrow (e.g.
 * "Torna a 'Studio 2026'"); when omitted callers should fall back
 * to the generic Common.back string.
 */
export type BackEntry = {
  route: Route
  context: NavContext
  label?: string
}

// Frozen sentinel so `consumePending()` can clear the slot without
// minting a fresh object every call.
const EMPTY_CONTEXT: NavContext = Object.freeze({})

function createRouter() {
  let current = $state<Route>('boot')
  // Bumps on every navigation. Destination screens that need to
  // react to *re*-navigation to the route they're already on (e.g.
  // the Projects screen reacting to a click on a different project
  // in the sidebar accordion while already on Projects) track this
  // signal. Without it Svelte short-circuits on `current = current`
  // and the destination's mount `$effect` never re-fires — first
  // surfaced on 2026-06-06 as "second sidebar project click does
  // nothing".
  let navTick = $state(0)
  // `pending` is intentionally a PLAIN variable, NOT `$state`. The
  // destination screen's mount `$effect` reads-and-clears it, so if
  // we made it reactive Svelte would subscribe the effect to the
  // pending signal, then the same effect's write (the clear) would
  // re-trigger it — that was the original
  // `effect_update_depth_exceeded`. We don't need reactivity on
  // this slot anyway: the navigation that sets it also bumps
  // `navTick`, which IS reactive, so destination effects re-run
  // and the read happens at the right moment.
  let pending: NavContext = EMPTY_CONTEXT
  let backStack = $state<BackEntry[]>([])

  return {
    get current() {
      return current
    },

    /**
     * Monotonically increasing tick — bumped by every `go()`,
     * `goWithReturn()` and `popBack()`. Destination `$effect`s that
     * need to re-fire on re-navigation to the SAME route (the same
     * sidebar-accordion-twice case) should reference this getter
     * so Svelte's tracker subscribes to it.
     */
    get navTick(): number {
      return navTick
    },

    /**
     * The top-of-stack back entry, or null if there's nothing to go
     * back to. Destination screens read this to decide whether to
     * surface a back button and what label to put on it.
     */
    get back(): BackEntry | null {
      return backStack.length > 0 ? backStack[backStack.length - 1] : null
    },

    /**
     * Standard navigation. **Clears the back stack** — the user picked
     * a sidebar entry / a deep-link directly, so they're starting a
     * fresh context and any leftover "back" target from an earlier
     * drill-down is no longer meaningful.
     */
    go(route: Route, context: NavContext = EMPTY_CONTEXT) {
      // Wrap mutations in `untrack` so callers running inside a
      // `$effect` body (App.svelte calls `router.go('boot')` from its
      // boot `$effect` for example) don't accidentally subscribe to
      // `navTick` or `backStack` via the read-modify-write under the
      // `++` and the `length` check. Without it the boot effect
      // re-fires every navigation — infinite loop on startup,
      // surfaced 2026-06-07 as `effect_update_depth_exceeded` before
      // the unlock screen could render.
      untrack(() => {
        current = route
        pending = context
        navTick = navTick + 1
        if (backStack.length > 0) backStack = []
      })
    },

    /**
     * Drill-down navigation. Pushes a back entry onto the stack so
     * the destination screen can offer a way back to the originating
     * context. Used e.g. by ProjectDetail when the user opens a chat:
     * the chat lands on Assistant, and Assistant shows a
     * "Torna a {progetto}" arrow that pops the stack.
     */
    goWithReturn(target: Route, targetContext: NavContext, entry: BackEntry) {
      untrack(() => {
        backStack = [...backStack, entry]
        current = target
        pending = targetContext
        navTick = navTick + 1
      })
    },

    /**
     * Pop the top back entry and navigate to it, restoring any
     * `context` the entry carried so the destination can rehydrate
     * its nested view (e.g. re-open the originating project detail).
     */
    popBack() {
      untrack(() => {
        if (backStack.length === 0) return
        const entry = backStack[backStack.length - 1]
        backStack = backStack.slice(0, -1)
        current = entry.route
        pending = entry.context
        navTick = navTick + 1
      })
    },

    /**
     * Routes read this once on mount and consume it (the call clears
     * the pending context so it isn't applied twice on rerenders).
     * Returns the frozen `EMPTY_CONTEXT` when nothing was queued.
     *
     * The implementation deliberately does NOT touch any reactive
     * state — see the `pending` declaration above for why.
     */
    consumePending(): NavContext {
      const ctx = pending
      pending = EMPTY_CONTEXT
      return ctx
    },
  }
}

export const router = createRouter()
