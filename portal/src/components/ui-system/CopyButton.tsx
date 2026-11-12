'use client';
import { useEffect, useRef, useState } from 'react';
export function CopyButton({ text, label = 'Copy command', className = 'copy-button' }: {
    text: string;
    label?: string;
    className?: string;
}) { const [state, setState] = useState(''), timer = useRef<ReturnType<typeof setTimeout> | null>(null); useEffect(() => () => { if (timer.current)
    clearTimeout(timer.current); }, []); return <button className={className} onClick={async () => { try {
    if (navigator.clipboard)
        await navigator.clipboard.writeText(text);
    else {
        const el = document.createElement('textarea');
        el.value = text;
        document.body.append(el);
        el.select();
        if (!document.execCommand('copy'))
            throw new Error('copy unavailable');
        el.remove();
    }
    setState('Copied ✓');
}
catch {
    setState('Select the command to copy');
} if (timer.current)
    clearTimeout(timer.current); timer.current = setTimeout(() => setState(''), 2200); }}><span aria-live="polite">{state || label}</span>{!state && ' ↗'}</button>; }
