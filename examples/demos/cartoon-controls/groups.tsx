import type { ReactNode } from "react";
import { Column, Row } from "@ipp/react/gui-kit";

export function Groups({
  wide,
  left,
  right,
}: {
  wide: boolean;
  left: ReactNode;
  right: ReactNode;
}) {
  if (!wide)
    return (
      <Column gap="field">
        {left}
        {right}
      </Column>
    );
  return (
    <Row gap="section" alignItems="start">
      <Column flex={1} gap="field">
        {left}
      </Column>
      <Column flex={1} gap="field">
        {right}
      </Column>
    </Row>
  );
}
