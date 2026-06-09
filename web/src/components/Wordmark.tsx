/** The FillerKiller wordmark — always white "Filler" + red "Killer", regardless
 *  of the surrounding text color. Use this anywhere the brand name is shown. */
export function Wordmark() {
  return (
    <span className="text-zinc-100">
      Filler<span className="text-rose-500">Killer</span>
    </span>
  );
}
