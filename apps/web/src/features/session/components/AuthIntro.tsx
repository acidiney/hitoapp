import type { ReactNode } from "react";
import { HitoLogo } from "../../../shared/brand/HitoLogo";

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
        <HitoLogo height={20} alwaysDark />
      </div>
      <div className="auth-message">
        <p className="auth-headline">{headline}</p>
        {detail && <p className="auth-detail">{detail}</p>}
      </div>
      <span className="auth-origin">{window.location.host}</span>
    </section>
  );
}
