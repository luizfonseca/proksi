import type { ButtonHTMLAttributes, ComponentChildren } from 'preact';
import { onLink } from '../../router';

const VARIANTS = {
  /** The page's main action */
  primary: 'btn btn-primary',
  /** Other actions, next to it */
  secondary: 'btn border-base-300 bg-base-100 font-medium shadow-none hover:bg-base-200',
  /** Removes something */
  danger: 'btn border-base-300 bg-base-100 font-medium text-error shadow-none hover:bg-error/10',
  /** A small action in a form: add, show more */
  link: 'link link-primary justify-self-start text-xs font-medium no-underline hover:underline',
  /** A small action that removes */
  'link-danger': 'link link-error justify-self-start text-xs font-medium no-underline hover:underline',
};

const SIZES = { xs: 'btn-xs', sm: 'btn-sm', md: '' };

type Props = {
  variant?: keyof typeof VARIANTS;
  size?: keyof typeof SIZES;
  /** A link to one of the UI's pages (or elsewhere, with `external`) instead of a button */
  href?: string;
  external?: boolean;
  /** Sends its form */
  submit?: boolean;
  class?: string;
  children: ComponentChildren;
} & Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'class' | 'size' | 'type'>;

export function Button({ variant = 'secondary', size = 'sm', href, external, submit, class: extra = '', children, ...rest }: Props) {
  const links = variant === 'link' || variant === 'link-danger';
  const style = `${VARIANTS[variant]} ${links ? '' : SIZES[size]} ${extra}`;
  if (href !== undefined) {
    return external ? (
      <a class={style} href={href} target="_blank" rel="noreferrer">
        {children}
      </a>
    ) : (
      <a class={style} href={href} onClick={onLink}>
        {children}
      </a>
    );
  }
  return (
    <button type={submit ? 'submit' : 'button'} class={style} {...rest}>
      {children}
    </button>
  );
}
