//! Machine model selection for the PMD 85 family.
//!
//! Only the four "basic" Tesla models are implemented; the enum keeps room
//! for the compatible machines (Consul 2717, Didaktik Alfa, Maťo) so they
//! can be added later without API churn.

/// The PMD 85 model to emulate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Model {
    /// Tesla PMD 85-1 (1984/85): 48 KiB RAM, 4 KiB ROM Monitor at 0x8000.
    Pmd851,
    /// Tesla PMD 85-2 (1986): same memory layout as 85-1, rewritten Monitor.
    Pmd852,
    /// Tesla PMD 85-2A (1987): 64 KiB RAM with extra pages at 0x9000/0xB000.
    Pmd852a,
    /// Tesla PMD 85-3 (1988): 64 KiB RAM, 8 KiB ROM Monitor at 0xE000,
    /// ROM/VRAM banking at 0xE000-0xFFFF. The default target.
    #[default]
    Pmd853,
}

impl Model {
    /// Default ROM file (in the `Rom/` directory) with the machine Monitor.
    pub fn default_monitor(self) -> &'static str {
        match self {
            Model::Pmd851 => "monit1.rom",
            Model::Pmd852 => "monit2.rom",
            Model::Pmd852a => "monit2A.rom",
            Model::Pmd853 => "monit3.rom",
        }
    }

    /// Canonical short name ("85-1" ... "85-3").
    pub fn name(self) -> &'static str {
        match self {
            Model::Pmd851 => "85-1",
            Model::Pmd852 => "85-2",
            Model::Pmd852a => "85-2A",
            Model::Pmd853 => "85-3",
        }
    }

    /// Parse from a CLI-style string.
    pub fn from_str_loose(s: &str) -> Option<Model> {
        match s.to_ascii_lowercase().as_str() {
            "1" | "85-1" | "pmd85-1" | "pmd851" => Some(Model::Pmd851),
            "2" | "85-2" | "pmd85-2" | "pmd852" => Some(Model::Pmd852),
            "2a" | "85-2a" | "pmd85-2a" | "pmd852a" => Some(Model::Pmd852a),
            "3" | "85-3" | "pmd85-3" | "pmd853" => Some(Model::Pmd853),
            _ => None,
        }
    }

    /// Address where the Monitor ROM is placed in memory, and its size.
    pub fn monitor_base(self) -> u16 {
        match self {
            Model::Pmd851 | Model::Pmd852 | Model::Pmd852a => 0x8000,
            Model::Pmd853 => 0xE000,
        }
    }

    pub fn monitor_size(self) -> usize {
        match self {
            Model::Pmd851 | Model::Pmd852 | Model::Pmd852a => 0x1000,
            Model::Pmd853 => 0x2000,
        }
    }
}
