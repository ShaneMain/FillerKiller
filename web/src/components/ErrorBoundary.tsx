import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}
interface State {
  error: Error | null;
}

/**
 * Catches render-time exceptions anywhere below it so a single component throw
 * shows a recoverable message instead of white-screening the whole SPA.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Surfaced to the console for local debugging; wire to an error reporter
    // (Sentry, etc.) here if/when one is added.
    console.error("Unhandled UI error:", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="mx-auto max-w-3xl px-4 py-20 text-center">
          <h1 className="text-2xl font-bold">Something went wrong</h1>
          <p className="mt-2 text-zinc-400">
            An unexpected error broke this page. Reloading usually fixes it.
          </p>
          <button
            onClick={() => window.location.assign("/")}
            className="mt-5 rounded-md bg-rose-600 px-4 py-2 font-medium text-white hover:bg-rose-500"
          >
            Reload FillerKiller
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
