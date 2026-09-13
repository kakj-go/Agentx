export function RequiredLabel({ children, required = false }: { children: React.ReactNode; required?: boolean }) {
  return <>{children}{required && <span aria-hidden="true" className="ml-1 text-danger">*</span>}</>;
}
