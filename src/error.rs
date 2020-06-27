use syn::Error;

#[derive(Default)]
pub struct ErrorRecord {
    error: Option<Error>,
}

impl ErrorRecord {
    pub fn add_error(&mut self, error: Error) {
        match &mut self.error {
            None => {
                self.error = Some(error);
            }
            Some(existing) => existing.combine(error),
        }
    }

    pub fn check(self) -> syn::Result<()> {
        match self.error {
            None => Ok(()),
            Some(e) => Err(e),
        }
    }
}
