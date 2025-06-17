//*** START FILE: src/profile_io.rs ***//
use crate::simulation::numerical_types::NumericalLearnerProfile;
use crate::simulation::dictionary::GlobalLemmaDictionary;
use serde::{Serialize, Deserialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Error as IoError, ErrorKind as IoErrorKind};
use std::path::Path;
use std::error::Error;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProfileSnapshot {
    pub profile: NumericalLearnerProfile,
    pub dictionary: GlobalLemmaDictionary,
}

pub fn save_profile_snapshot(
    profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
    file_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let snapshot = ProfileSnapshot {
        profile: profile.clone(), 
        dictionary: dictionary.clone(),
    };
    let file = File::create(file_path)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, &snapshot)?;
    Ok(())
}

pub fn load_profile_snapshot(
    file_path: &Path,
) -> Result<(NumericalLearnerProfile, GlobalLemmaDictionary), Box<dyn Error>> {
    if !file_path.exists() {
        return Err(Box::new(IoError::new(
            IoErrorKind::NotFound,
            format!("Profile snapshot file not found at {:?}", file_path),
        )));
    }
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);
    let snapshot: ProfileSnapshot = serde_json::from_reader(reader)?;
    Ok((snapshot.profile, snapshot.dictionary))
}
//*** END FILE: src/profile_io.rs ***//