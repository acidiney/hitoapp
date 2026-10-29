import type { ReactNode, Ref } from "react";

export default function PageHeading({
  title,
  description,
  headingRef,
  children,
}: {
  title: string;
  description: string;
  headingRef?: Ref<HTMLHeadingElement>;
  children?: ReactNode;
}) {
  return (
    <div className="page-heading">
      <div>
        <h1 ref={headingRef} tabIndex={headingRef ? -1 : undefined}>
          {title}
        </h1>
        <p className="muted">{description}</p>
      </div>
      {children}
    </div>
  );
}
