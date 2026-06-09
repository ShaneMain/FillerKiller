import { usePageMeta } from "../lib/meta";

const KOFI_URL = "https://ko-fi.com/shanemain";

export function SupportPage() {
  usePageMeta(
    "Support",
    "FillerKiller is free and open source — support its hosting and development.",
  );

  return (
    <div className="mx-auto max-w-2xl px-4 py-12 text-center">
      <h1 className="text-3xl font-bold">Support FillerKiller</h1>
      <p className="mx-auto mt-4 max-w-prose text-zinc-300">
        FillerKiller is free, open source, and ad-free. It runs on a small budget for hosting.
        If it's saved you from a few hours of filler, you can chip in to keep it online and
        growing — entirely optional, and never required to vote.
      </p>

      <a
        href={KOFI_URL}
        target="_blank"
        rel="noreferrer"
        className="mt-8 inline-flex items-center gap-2 rounded-md bg-rose-600 px-5 py-2.5 font-medium text-white hover:bg-rose-500"
      >
        ☕ Support on Ko-fi
      </a>

      <p className="mt-6 text-sm text-zinc-500">
        Prefer to help another way? Contributions are welcome on{" "}
        <a
          href="https://github.com/ShaneMain/FillerKiller"
          target="_blank"
          rel="noreferrer"
          className="underline hover:text-zinc-300"
        >
          GitHub
        </a>
        .
      </p>
    </div>
  );
}
