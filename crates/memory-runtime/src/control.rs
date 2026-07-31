//! Runtime 的进程内暂停、恢复、取消和用户输入控制。

/// Runtime 控制状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlState {
    /// 正常运行。
    Running,
    /// 等待用户输入。
    WaitingUser,
    /// 已暂停。
    Paused,
    /// 已取消。
    Cancelled,
}

/// 控制操作失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ControlError {
    /// 当前状态不允许该操作。
    #[error("invalid control state")]
    InvalidState,
    /// 运行已经取消。
    #[error("runtime is terminal")]
    Terminal,
}

/// 可暂停、恢复和接收用户输入的 Runtime 控制器。
pub struct RunControl {
    state: ControlState,
    before_pause: ControlState,
    user_input: Option<String>,
}

impl RunControl {
    /// 创建运行中的控制器。
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: ControlState::Running,
            before_pause: ControlState::Running,
            user_input: None,
        }
    }
    /// 返回当前控制状态。
    #[must_use]
    pub const fn state(&self) -> ControlState {
        self.state
    }
    /// 进入等待用户状态。
    ///
    /// # Errors
    ///
    /// 已取消时返回终态错误。
    pub fn wait_for_user(&mut self) -> Result<(), ControlError> {
        self.ensure_live()?;
        self.state = ControlState::WaitingUser;
        Ok(())
    }
    /// 提供用户输入并恢复运行。
    ///
    /// # Errors
    ///
    /// 当前状态不是等待用户时返回状态错误。
    pub fn provide_user_input(&mut self, input: impl Into<String>) -> Result<(), ControlError> {
        if self.state != ControlState::WaitingUser {
            return Err(ControlError::InvalidState);
        }
        self.user_input = Some(input.into());
        self.state = ControlState::Running;
        Ok(())
    }
    /// 取出最近一次用户输入。
    pub fn take_user_input(&mut self) -> Option<String> {
        self.user_input.take()
    }
    /// 暂停运行。
    ///
    /// # Errors
    ///
    /// 已取消或已经暂停时返回错误。
    pub fn pause(&mut self) -> Result<(), ControlError> {
        self.ensure_live()?;
        if self.state == ControlState::Paused {
            return Err(ControlError::InvalidState);
        }
        self.before_pause = self.state;
        self.state = ControlState::Paused;
        Ok(())
    }
    /// 从暂停点恢复。
    ///
    /// # Errors
    ///
    /// 当前状态不是暂停时返回错误。
    pub fn resume(&mut self) -> Result<(), ControlError> {
        if self.state != ControlState::Paused {
            return Err(ControlError::InvalidState);
        }
        self.state = self.before_pause;
        Ok(())
    }
    /// 取消运行并进入终态。
    ///
    /// # Errors
    ///
    /// 已取消时返回终态错误。
    pub fn cancel(&mut self) -> Result<(), ControlError> {
        self.ensure_live()?;
        self.state = ControlState::Cancelled;
        Ok(())
    }
    fn ensure_live(&self) -> Result<(), ControlError> {
        if self.state == ControlState::Cancelled {
            Err(ControlError::Terminal)
        } else {
            Ok(())
        }
    }
}

impl Default for RunControl {
    fn default() -> Self {
        Self::new()
    }
}
