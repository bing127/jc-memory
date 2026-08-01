import { forwardRef, type InputHTMLAttributes } from "react";

import { cn } from "../../lib/utils.js";

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  ({ className, ...props }, ref) => (
    <input
      ref={ref}
      className={cn(
        "h-10 w-full rounded-[5px] border border-[#cfd5d0] bg-white px-3 text-sm text-[#171a18] outline-none transition-shadow placeholder:text-[#8a938d] focus:border-[#38a169] focus:ring-2 focus:ring-[#38c172]/20 disabled:bg-[#f2f4f2]",
        className,
      )}
      {...props}
    />
  ),
);
Input.displayName = "Input";
