// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//
// ESLint flat config for the Svelte 5 + TypeScript frontend.
//
// `svelte-check` (npm run typecheck) is the primary gate — full type
// checking + the Svelte compiler. ESLint complements it with rules the
// type checker doesn't cover: Svelte-specific correctness and a few
// JS/TS foot-guns. Style is owned by Prettier, so purely stylistic
// rules are left off here.

import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import svelte from 'eslint-plugin-svelte'
import globals from 'globals'

export default tseslint.config(
  { ignores: ['dist/', 'build/', '.svelte-kit/', 'node_modules/'] },

  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...svelte.configs['flat/recommended'],

  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
    },
  },

  // `<script lang="ts">` blocks are parsed by the TS parser.
  {
    files: ['**/*.svelte', '**/*.svelte.ts'],
    languageOptions: {
      parserOptions: { parser: tseslint.parser },
    },
  },

  {
    rules: {
      // The app renders Markdown/HTML through DOMPurify before
      // `{@html}` — the sink is sanitised by design.
      'svelte/no-at-html-tags': 'off',
      // svelte-check already runs the Svelte compiler (it is the
      // `build` gate); re-running it inside ESLint is redundant and
      // also surfaces irrelevant custom-element warnings (this is a
      // normal app, not a custom-element library).
      'svelte/valid-compile': 'off',
      // `<!-- svelte-ignore -->` comments are compiler directives;
      // this rule mis-reports them as unused.
      'svelte/no-unused-svelte-ignore': 'off',
      // Name resolution is TypeScript's job (and svelte-check's) —
      // core no-undef false-positives on TS generics in `.svelte`.
      'no-undef': 'off',
      // Unused vars: allow a leading underscore for deliberately
      // unused bindings (e.g. destructured-but-ignored values).
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
    },
  },
)
