import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'

export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      globals: globals.browser,
    },
    rules: {
      // Data-loading effects legitimately reset/clear state before an async fetch
      // and set it on mount; the react-hooks v7 rule is over-eager about that
      // (fetch-in-effect is a documented, valid pattern). Keep it visible as a
      // warning rather than a CI-blocking error.
      'react-hooks/set-state-in-effect': 'warn',
      // Dev-only Fast Refresh hint (the auth module exports a hook beside its
      // provider). No production impact; keep as a warning.
      'react-refresh/only-export-components': 'warn',
    },
  },
])
