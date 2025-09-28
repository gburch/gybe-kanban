import { useEffect } from 'react';

const FOCUSABLE_TAGS = new Set(['INPUT', 'TEXTAREA', 'SELECT']);

const isIOS = () => {
  if (typeof navigator === 'undefined') return false;

  const userAgent = navigator.userAgent || navigator.vendor;
  const platform = navigator.platform;
  const isAppleTouchDevice =
    /iPad|iPhone|iPod/.test(userAgent) ||
    (platform === 'MacIntel' && navigator.maxTouchPoints > 1);

  return isAppleTouchDevice;
};

const isFormControl = (element: Element | null): element is HTMLElement => {
  if (!element) return false;
  return FOCUSABLE_TAGS.has(element.tagName);
};

const withViewportParam = (
  content: string,
  param: string,
  value: string
) => {
  const pattern = new RegExp(`${param}\\s*=\\s*[^,]+`, 'i');
  if (pattern.test(content)) {
    return content.replace(pattern, `${param}=${value}`);
  }
  const separator = content.trim().endsWith(',') ? ' ' : ', ';
  return `${content}${separator}${param}=${value}`;
};

export const usePreventInputZoom = () => {
  useEffect(() => {
    if (!isIOS()) return;

    const viewport = document.querySelector('meta[name="viewport"]');
    if (!viewport) return;

    const originalContent =
      viewport.getAttribute('content')?.trim() ||
      'width=device-width, initial-scale=1.0';
    const noZoomContent = [
      { param: 'maximum-scale', value: '1.0' },
      { param: 'user-scalable', value: '0' },
    ].reduce(
      (current, { param, value }) => withViewportParam(current, param, value),
      originalContent || 'width=device-width, initial-scale=1.0'
    );

    const disableZoom = (event: FocusEvent) => {
      const target = event.target as HTMLElement | null;
      if (!isFormControl(target)) return;
      viewport.setAttribute('content', noZoomContent);
    };

    const restoreZoom = (event: FocusEvent) => {
      const target = event.target as HTMLElement | null;
      if (!isFormControl(target)) return;

      requestAnimationFrame(() => {
        const active = document.activeElement;
        if (isFormControl(active)) return;
        viewport.setAttribute('content', originalContent);
      });
    };

    document.addEventListener('focusin', disableZoom);
    document.addEventListener('focusout', restoreZoom);

    return () => {
      viewport.setAttribute('content', originalContent);
      document.removeEventListener('focusin', disableZoom);
      document.removeEventListener('focusout', restoreZoom);
    };
  }, []);
};
