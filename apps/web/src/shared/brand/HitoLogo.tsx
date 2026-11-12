import logoOnDark from "./hito-logo.svg";
import logoOnLight from "./hito-logo-on-light.svg";
import markOnDark from "./hito-mark.svg";
import markOnLight from "./hito-mark-on-light.svg";

const darkScheme = "(prefers-color-scheme: dark)";

export function HitoLogo({
  height = 15,
  alwaysDark = false,
}: {
  height?: number;
  alwaysDark?: boolean;
}) {
  if (alwaysDark)
    return (
      <picture className="hito-logo">
        <img src={logoOnDark} alt="hito" height={height} />
      </picture>
    );
  return (
    <picture className="hito-logo">
      <source media={darkScheme} srcSet={logoOnDark} />
      <img src={logoOnLight} alt="hito" height={height} />
    </picture>
  );
}

export function HitoMark({ dot = 10 }: { dot?: number }) {
  return (
    <picture className="hito-mark">
      <source media={darkScheme} srcSet={markOnDark} />
      <img src={markOnLight} alt="hito" height={dot} />
    </picture>
  );
}
