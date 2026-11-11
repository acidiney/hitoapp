import type { ReactNode } from "react";
import PageHeading from "../shared/ui/PageHeading";
import LibrarySections from "./LibrarySections";

export default function LibraryHeading({
  path,
  title,
  description,
  children,
}: {
  path: string;
  title: string;
  description?: string;
  children?: ReactNode;
}) {
  return (
    <div className="library-heading">
      <PageHeading title={title} description={description}>
        {children}
      </PageHeading>
      <LibrarySections path={path} />
    </div>
  );
}
