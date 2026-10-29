import type { ReactNode } from "react";
import BrandMark from "../../../shared/ui/BrandMark";

export default function AuthIntro({
  headline,
  decorative = false,
}: {
  headline: ReactNode;
  decorative?: boolean;
}) {
  return (
    <section className="auth-intro" aria-hidden={decorative || undefined}>
      <div className="brand">
        <BrandMark />
        <span className="brand-word">hito</span>
      </div>
      <p className="auth-headline">{headline}</p>
      <span className="auth-origin">{window.location.host}</span>
    </section>
  );
}
