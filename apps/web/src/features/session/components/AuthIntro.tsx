import type { ReactNode } from "react";
import BrandMark from "../../../shared/ui/BrandMark";

export default function AuthIntro({
  headline,
  detail,
  tone = "ink",
  decorative = false,
}: {
  headline: ReactNode;
  detail?: ReactNode;
  tone?: "ink" | "action";
  decorative?: boolean;
}) {
  return (
    <section
      className="auth-intro"
      data-tone={tone}
      aria-hidden={decorative || undefined}
    >
      <div className="brand">
        <BrandMark />
        <span className="brand-word">hito</span>
      </div>
      <div className="auth-message">
        <p className="auth-headline">{headline}</p>
        {detail && <p className="auth-detail">{detail}</p>}
      </div>
      <span className="auth-origin">{window.location.host}</span>
    </section>
  );
}
