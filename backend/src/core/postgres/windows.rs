use std::{
    ffi::OsStr,
    fs::File,
    io,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    process::Command,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Security::{
        CreateRestrictedToken, CreateWellKnownSid, DISABLE_MAX_PRIVILEGE, SID_AND_ATTRIBUTES,
        TOKEN_ALL_ACCESS, WinBuiltinAdministratorsSid, WinBuiltinPowerUsersSid,
    },
    System::{
        JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
        Threading::{
            CREATE_NO_WINDOW, CreateProcessAsUserW, DeleteProcThreadAttributeList,
            EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, InitializeProcThreadAttributeList,
            LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcessToken, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
            STARTUPINFOEXW, STARTUPINFOW, UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};

pub(super) struct Process {
    _job: OwnedHandle,
    handle: OwnedHandle,
    pid: u32,
}

impl Process {
    pub(super) fn id(&self) -> u32 {
        self.pid
    }

    pub(super) fn try_wait(&mut self) -> io::Result<Option<()>> {
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => Ok(Some(())),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

fn checked(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn inheritable(file: &File) -> io::Result<OwnedHandle> {
    let mut handle = null_mut();
    unsafe {
        let current = GetCurrentProcess();
        checked(DuplicateHandle(
            current,
            file.as_raw_handle(),
            current,
            &mut handle,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        ))?;
        Ok(OwnedHandle::from_raw_handle(handle))
    }
}

fn restricted_token() -> io::Result<OwnedHandle> {
    let mut original = null_mut();
    // Match PostgreSQL's requirement to run without the administrator and power-user groups.
    let mut sid_storage = [[0u32; 17]; 2];
    let mut groups = [SID_AND_ATTRIBUTES::default(); 2];
    unsafe {
        checked(OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ALL_ACCESS,
            &mut original,
        ))?;
        let original = OwnedHandle::from_raw_handle(original);
        for ((group, storage), kind) in groups
            .iter_mut()
            .zip(&mut sid_storage)
            .zip([WinBuiltinAdministratorsSid, WinBuiltinPowerUsersSid])
        {
            let mut size = size_of::<[u32; 17]>() as u32;
            group.Sid = storage.as_mut_ptr().cast();
            checked(CreateWellKnownSid(kind, null_mut(), group.Sid, &mut size))?;
        }
        let mut restricted = null_mut();
        checked(CreateRestrictedToken(
            original.as_raw_handle(),
            DISABLE_MAX_PRIVILEGE,
            groups.len() as u32,
            groups.as_ptr(),
            0,
            null(),
            0,
            null(),
            &mut restricted,
        ))?;
        Ok(OwnedHandle::from_raw_handle(restricted))
    }
}

fn command_line(command: &Command) -> io::Result<Vec<u16>> {
    let quote = u16::from(b'"');
    let slash = u16::from(b'\\');
    let mut line = Vec::new();
    for arg in std::iter::once(command.get_program()).chain(command.get_args()) {
        if !line.is_empty() {
            line.push(u16::from(b' '));
        }
        line.push(quote);
        let mut trailing_slashes = 0;
        for ch in arg.encode_wide() {
            if ch == quote || ch == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid PostgreSQL path",
                ));
            }
            trailing_slashes = if ch == slash { trailing_slashes + 1 } else { 0 };
            line.push(ch);
        }
        line.extend(std::iter::repeat_n(slash, trailing_slashes));
        line.push(quote);
    }
    line.push(0);
    Ok(line)
}

struct Attributes(Vec<usize>);

impl Attributes {
    fn new() -> io::Result<Self> {
        let mut bytes = 0;
        unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes) };
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        checked(unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 2, 0, &mut bytes)
        })?;
        Ok(Self(storage))
    }

    fn as_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.0.as_mut_ptr().cast()
    }

    fn set(&mut self, attribute: u32, handles: &[HANDLE]) -> io::Result<()> {
        checked(unsafe {
            UpdateProcThreadAttribute(
                self.as_ptr(),
                0,
                attribute as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(handles),
                null_mut(),
                null(),
            )
        })
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.as_ptr()) };
    }
}

pub(super) fn spawn(command: &Command, log: &File) -> io::Result<Process> {
    let token = restricted_token()?;
    let input = inheritable(&File::open("NUL")?)?;
    let log = inheritable(log)?;
    let program: Vec<u16> = OsStr::encode_wide(command.get_program())
        .chain([0])
        .collect();
    let mut args = command_line(command)?;
    // Assign the job at creation so an interrupted launch cannot leave an orphan.
    unsafe {
        let job = CreateJobObjectW(null(), null());
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = OwnedHandle::from_raw_handle(job);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        checked(SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ))?;
        let inherited = [input.as_raw_handle(), log.as_raw_handle()];
        let jobs = [job.as_raw_handle()];
        let mut attributes = Attributes::new()?;
        attributes.set(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &inherited)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
        let startup = STARTUPINFOEXW {
            StartupInfo: STARTUPINFOW {
                cb: size_of::<STARTUPINFOEXW>() as u32,
                dwFlags: STARTF_USESTDHANDLES,
                hStdInput: input.as_raw_handle(),
                hStdOutput: log.as_raw_handle(),
                hStdError: log.as_raw_handle(),
                ..zeroed()
            },
            lpAttributeList: attributes.as_ptr(),
        };
        let mut process: PROCESS_INFORMATION = zeroed();
        checked(CreateProcessAsUserW(
            token.as_raw_handle(),
            program.as_ptr(),
            args.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
            null(),
            null(),
            &startup.StartupInfo,
            &mut process,
        ))?;
        let handle = OwnedHandle::from_raw_handle(process.hProcess);
        drop(OwnedHandle::from_raw_handle(process.hThread));
        Ok(Process {
            _job: job,
            handle,
            pid: process.dwProcessId,
        })
    }
}
