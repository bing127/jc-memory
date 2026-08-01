import { cva, type VariantProps } from "class-variance-authority";
import {
  forwardRef,
  type ButtonHTMLAttributes,
} from "react";

import { cn } from "../../lib/utils.js";

const buttonVariants = cva(
  "inline-flex h-10 items-center justify-center gap-2 rounded-[5px] px-4 text-sm font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#38c172] focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        primary: "bg-[#101311] text-white hover:bg-[#2a2f2b]",
        secondary: "border border-[#d8ddd9] bg-white text-[#171a18] hover:bg-[#f2f5f2]",
        ghost: "text-[#4f5852] hover:bg-[#eef1ee] hover:text-[#101311]",
        danger: "bg-[#b42318] text-white hover:bg-[#8f1c13]",
      },
      size: {
        default: "h-10 px-4",
        icon: "size-10 p-0",
        compact: "h-8 px-3 text-xs",
      },
    },
    defaultVariants: { variant: "primary", size: "default" },
  },
);

export interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, ...props }, ref) => (
    <button
      ref={ref}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  ),
);
Button.displayName = "Button";
