import type { ReactNode } from 'react';
import { RouteEntrance } from '@/generated/motion';

export function PageIntro({ title, description, children }: {
    title: string;
    description: string;
    children?: ReactNode;
}) {
    return <><RouteEntrance /><section className="page-intro section slp-on-blue" data-surface="blue">
        <h1>{title}</h1>
        <div className="page-intro-bottom"><p>{description}</p></div>
        {children && <div className="page-intro-actions">{children}</div>}
    </section></>;
}
