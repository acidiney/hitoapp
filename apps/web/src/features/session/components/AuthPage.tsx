import type { ReactNode } from "react";
import { HitoLogo } from "../../../shared/brand/HitoLogo";

export default function AuthPage({ children }: { children: ReactNode }) {
  return (
    <main className="auth-page">
      <span className="auth-logo">
        <HitoLogo height={19} />
      </span>
      {children}
    </main>
  );
}
