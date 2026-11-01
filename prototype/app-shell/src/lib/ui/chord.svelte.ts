// PROTOTYPE — throwaway. The first key of a chord (Ctrl+K …) while gh-rev waits for the second, for the
// status bar. Set by keymap().
export const chord = $state<{ pending: string | null }>({ pending: null });
