import type { FrameLook } from "./model";

/** A shot's picture until the app renders the real one: sky, sea, the sun
 *  and one silhouette. It fills its box, so the box sets the aspect. */
export function Frame({ look, className = "" }: { look: FrameLook; className?: string }) {
  const { horizon, sun, subject, at } = look;
  const h = `${horizon * 100}%`;
  return (
    <div
      className={`relative overflow-hidden ${className}`}
      style={{
        background: `linear-gradient(#2c2c5a 0%, #9a4f78 ${horizon * 62}%, #f0a25e ${horizon * 100}%, #1f2c4c ${horizon * 100}%, #0d1424 100%)`,
      }}
    >
      <div
        className="absolute rounded-full"
        style={{
          left: `${sun[0] * 100}%`,
          top: `${sun[1] * 100}%`,
          width: "14%",
          aspectRatio: "1",
          translate: "-50% -50%",
          background: "radial-gradient(#ffe2b0 0 40%, #ffbf7a66 60%, transparent 70%)",
        }}
      />
      {subject !== "none" && <Silhouette subject={subject} at={at} horizon={h} />}
    </div>
  );
}

function Silhouette({ subject, at, horizon }: { subject: Exclude<FrameLook["subject"], "none">; at: number; horizon: string }) {
  const left = `${at * 100}%`;
  const ink = "#0a0c14";
  switch (subject) {
    case "tower":
      return (
        <>
          <div className="absolute" style={{ left, bottom: `calc(100% - ${horizon})`, width: "5%", height: "48%", background: ink, translate: "-50% 0" }} />
          <div className="absolute rounded-full" style={{ left, bottom: `calc(100% - ${horizon} + 46%)`, width: "4%", aspectRatio: "1", background: "#ffe9a8", translate: "-50% 0" }} />
        </>
      );
    case "boat":
      return (
        <div
          className="absolute"
          style={{ left, top: `calc(${horizon} + 6%)`, width: "26%", height: "14%", background: ink, translate: "-50% 0", clipPath: "polygon(0 0, 100% 0, 84% 100%, 12% 100%)" }}
        />
      );
    case "lamp":
      return (
        <>
          <div className="absolute" style={{ left, bottom: "0", width: "1.6%", height: "86%", background: ink, translate: "-50% 0" }} />
          <div className="absolute rounded-full" style={{ left, top: "10%", width: "9%", aspectRatio: "1", background: "radial-gradient(#fff1c4 0 30%, #ffd16655 55%, transparent 70%)", translate: "-50% -40%" }} />
        </>
      );
    case "jetty":
      return <div className="absolute" style={{ left: 0, width: `${at * 100}%`, top: `calc(${horizon} + 14%)`, height: "9%", background: ink }} />;
  }
}
