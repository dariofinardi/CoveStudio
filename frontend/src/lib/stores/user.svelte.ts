// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { userApi } from '$lib/api/user'
import { i18n } from '$lib/stores/i18n.svelte'
import { DEFAULT_DOMAIN, DOMAINS, type Domain } from '$lib/types/domain'
import { DEFAULT_LOCALE, type Locale, type UserProfile } from '$lib/types/user'

/**
 * User profile + preferences (locale, default domain). Hydrated after a
 * successful unlock; the auth store owns the session identity, this one
 * owns everything persisted in `user_settings`.
 */
function createUserStore() {
  let profile = $state<UserProfile | null>(null)
  let locale = $state<Locale>(DEFAULT_LOCALE)
  let defaultDomain = $state<Domain>(DEFAULT_DOMAIN)
  // `null` = no explicit preference → every domain is enabled. An
  // explicit list lets settings UI filter selectors without forcing
  // every caller to special-case the "all enabled" sentinel.
  let enabledDomains = $state<Domain[] | null>(null)
  let loading = $state<boolean>(false)

  return {
    get profile() {
      return profile
    },
    get locale() {
      return locale
    },
    get defaultDomain() {
      return defaultDomain
    },
    /** Raw preference: `null` = no explicit set (every domain enabled). */
    get enabledDomains() {
      return enabledDomains
    },
    /** Always returns a concrete list — uses every shipped domain when no preference is set. */
    get effectiveEnabledDomains(): Domain[] {
      return enabledDomains ?? (DOMAINS as readonly Domain[]).slice()
    },
    get loading() {
      return loading
    },

    isDomainEnabled(d: Domain): boolean {
      return enabledDomains === null || enabledDomains.includes(d)
    },

    /** Pull profile + preferences in one shot (post-unlock). */
    async hydrate() {
      loading = true
      try {
        const [prof, loc, dom, ed] = await Promise.all([
          userApi.getProfile(),
          userApi.getLocale(),
          userApi.getDefaultDomain(),
          userApi.getEnabledDomains(),
        ])
        profile = prof
        locale = loc.locale ?? DEFAULT_LOCALE
        defaultDomain = dom.default_domain ?? DEFAULT_DOMAIN
        enabledDomains = ed.enabled_domains
        i18n.setLocale(locale)
      } finally {
        loading = false
      }
    },

    async setLocale(next: Locale) {
      const res = await userApi.updateLocale(next)
      locale = res.locale
      i18n.setLocale(res.locale)
    },

    async setDefaultDomain(next: Domain) {
      const res = await userApi.updateDefaultDomain(next)
      defaultDomain = res.default_domain
    },

    async setEnabledDomains(next: Domain[]) {
      const res = await userApi.updateEnabledDomains(next)
      enabledDomains = res.enabled_domains
    },

    async setDisplayName(name: string | null) {
      await userApi.updateProfile(name)
      if (profile) profile = { ...profile, display_name: name }
    },

    reset() {
      profile = null
      locale = DEFAULT_LOCALE
      defaultDomain = DEFAULT_DOMAIN
      enabledDomains = null
    },
  }
}

export const userStore = createUserStore()
