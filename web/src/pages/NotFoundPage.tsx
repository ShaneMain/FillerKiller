import { Link } from "react-router-dom";

export function NotFoundPage() {
  return (
    <div className="mx-auto max-w-3xl px-4 py-20 text-center">
      <h1 className="text-3xl font-bold">Page not found</h1>
      <p className="mt-2 text-zinc-400">That page doesn't exist.</p>
      <Link to="/" className="mt-5 inline-block font-medium text-rose-400 hover:text-rose-300">
        ← Back to search
      </Link>
    </div>
  );
}
