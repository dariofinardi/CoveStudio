// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { mount } from 'svelte'
import App from './App.svelte'
import { themeStore } from '$lib/stores/theme.svelte'
import { i18n } from '$lib/stores/i18n.svelte'
import './app.css'

const target = document.getElementById('app')
if (!target) throw new Error('Root #app element missing from index.html')

// Apply the persisted theme before mounting to avoid a flash of the
// wrong palette.
themeStore.init()

// Initial UI locale guess from the OS; the persisted per-user locale
// is applied after unlock when the user store hydrates.
i18n.detectFrom(navigator.language)

export const app = mount(App, { target })
