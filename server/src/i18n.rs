//! Server messages in the app's language (audit M13). The app sends `Accept-Language`; for Portuguese, every
//! error message the server writes in English is answered from this table. Messages stay in English in the code
//! (and the log), so there is one place to translate. A unit test fails if a message in the code has no translation.

use axum::{body::Body, extract::Request, http::header, middleware::Next, response::Response};

/// (English, pt-PT). Keep it sorted; add a row whenever a new `AppError` message appears.
pub const PT: &[(&str, &str)] = &[
    ("A closure can be at most three months. Add another one for longer.",
     "Um encerramento pode ter no máximo três meses. Para mais tempo, acrescente outro."),
    ("A month can be closed once it has ended.",
     "Um mês só pode ser fechado depois de terminar."),
    ("A recording can be at most 2 h 30. Stop and start a new one.",
     "Uma gravação pode ter no máximo 2 h 30. Pare e comece uma nova."),
    ("A withdrawn consent stays withdrawn. Ask the patient to sign it again if needed.",
     "Um consentimento retirado fica retirado. Se for preciso, peça ao utente que o assine de novo."),
    ("A voided payment stays voided. Record the payment again if needed.",
     "Um pagamento anulado fica anulado. Se for preciso, registe o pagamento outra vez."),
    ("Add what is being paid.",
     "Indique o que está a ser pago."),
    ("An account with this email already exists. Sign in instead.",
     "Já existe uma conta com este email. Entre em vez disso."),
    ("An answer is out of range. Check the form.",
     "Há uma resposta fora do intervalo. Reveja o formulário."),
    ("Answer every question.",
     "Responda a todas as perguntas."),
    ("Answer the questions first.",
     "Responda primeiro às perguntas."),
    ("Approve the letter first.",
     "Aprove a carta primeiro."),
    ("Approved letters are kept.",
     "As cartas aprovadas são mantidas."),
    ("Attach the invoice PDF again.",
     "Anexe de novo o PDF da fatura."),
    ("Before publishing, fill in the clinic’s name, NIF and a phone or email in Settings › Clinic.",
     "Antes de publicar, preencha o nome da clínica, o NIF e um telefone ou email em Definições › Clínica."),
    ("Can’t reach the local AI. Check the AI computer is on and Ollama is running, and its address in Settings › Local AI.",
     "Sem resposta da IA local. Verifique se o computador da IA está ligado e o Ollama a correr, e o endereço em Definições › IA local."),
    ("Check the Social Security amount.",
     "Confirme o valor da Segurança Social."),
    ("Check the amount.",
     "Verifique o valor."),
    ("Check the counted amounts.",
     "Confirme os valores contados."),
    ("Check the email address.",
     "Confirme o endereço de email."),
    ("Check the payment date.",
     "Confirme a data do pagamento."),
    ("Check the phone number.",
     "Confirme o número de telefone."),
    ("Choose Portuguese or English.",
     "Escolha português ou inglês."),
    ("Choose a category.",
     "Escolha uma categoria."),
    ("Choose a day.",
     "Escolha um dia."),
    ("Choose a full or summary export.",
     "Escolha a exportação completa ou o resumo."),
    ("Choose a package or monthly plan.",
     "Escolha um pacote ou plano mensal."),
    ("Choose a password that’s different from the current one.",
     "Escolha uma palavra-passe diferente da atual."),
    ("Choose a password with at least 12 characters.",
     "Escolha uma palavra-passe com pelo menos 12 caracteres."),
    ("Choose a patient or enter a name and phone.",
     "Escolha um utente ou escreva o nome e o telefone."),
    ("Choose a period of up to two months.",
     "Escolha um período até dois meses."),
    ("Choose a role.",
     "Escolha uma função."),
    ("Choose a status.",
     "Escolha um estado."),
    ("Choose a test and enter its score.",
     "Escolha um teste e escreva a pontuação."),
    ("Choose a valid date.",
     "Escolha uma data válida."),
    ("Choose an end date or a number of sessions.",
     "Escolha uma data final ou um número de sessões."),
    ("Choose how it was paid.",
     "Escolha como foi pago."),
    ("Choose how it was paid: cash, MB WAY, transfer or card.",
     "Escolha como foi pago: numerário, MB WAY, transferência ou cartão."),
    ("Choose how the reminder was sent.",
     "Escolha como o lembrete foi enviado."),
    ("Choose someone else to share it with.",
     "Escolha outra pessoa com quem partilhar."),
    ("Choose the days of the week to repeat on.",
     "Escolha os dias da semana em que se repete."),
    ("Choose the days of the week.",
     "Escolha os dias da semana."),
    ("Choose the first day.",
     "Escolha o primeiro dia."),
    ("Choose the last day.",
     "Escolha o último dia."),
    ("Choose the package.",
     "Escolha o pacote."),
    ("Choose the patient.",
     "Escolha o utente."),
    ("Choose the scanned prescription again.",
     "Escolha de novo a prescrição digitalizada."),
    ("Choose the therapist.",
     "Escolha o fisioterapeuta."),
    ("Choose the type of appointment.",
     "Escolha o tipo de marcação."),
    ("Choose the type: visit, package or monthly plan.",
     "Escolha o tipo: sessão avulsa, pacote ou plano mensal."),
    ("Choose today or a later day.",
     "Escolha hoje ou um dia a seguir."),
    ("Choose up to two years, with the start before the end.",
     "Escolha até dois anos, com o início antes do fim."),
    ("Choose what this file is.",
     "Escolha o que é este ficheiro."),
    ("Choose where the complaint came from.",
     "Escolha por onde chegou a reclamação."),
    ("Choose which appointment this price is for.",
     "Escolha a que tipo de marcação se aplica este preço."),
    ("Choose which appointments to change.",
     "Escolha que marcações alterar."),
    ("Choose who to share it with.",
     "Escolha com quem partilhar."),
    ("Commission and room rental aren’t available yet. Use a fixed amount.",
     "A comissão e o aluguer de sala ainda não estão disponíveis. Use um valor fixo."),
    ("Describe what is being paid.",
     "Descreva o que está a ser pago."),
    ("Each day needs an opening time before its closing time, like 08:00–20:00, or Closed.",
     "Cada dia precisa de uma hora de abertura antes da de fecho, como 08:00–20:00, ou Fechado."),
    ("Enter a name and a phone or email.",
     "Escreva o nome e um telefone ou email."),
    ("Enter a title and the text.",
     "Escreva o título e o texto."),
    ("Enter how many people fit in the class.",
     "Indique quantas pessoas cabem na aula."),
    ("Enter the date and the amount.",
     "Indique a data e o valor."),
    ("Enter the name in Portuguese and in English.",
     "Escreva o nome em português e em inglês."),
    ("Enter the name of the person signing.",
     "Escreva o nome de quem assina."),
    ("Enter the patient’s name.",
     "Escreva o nome do utente."),
    ("Enter the prescription date and the number of sessions.",
     "Indique a data da prescrição e o número de sessões."),
    ("Enter the price.",
     "Indique o preço."),
    ("Files must be under 10 MB.",
     "Os ficheiros têm de ter menos de 10 MB."),
    ("Fill in the clinic name, your name and a valid email.",
     "Preencha o nome da clínica, o seu nome e um email válido."),
    ("Fill in the note first: the AI only rewrites what’s there.",
     "Preencha a nota primeiro: a IA só reescreve o que lá está."),
    ("Fill in the parts marked [a completar] before approving.",
     "Complete as partes marcadas [a completar] antes de aprovar."),
    ("Fill in your name and a valid email.",
     "Preencha o seu nome e um email válido."),
    ("Give the device a name (up to 60 characters).",
     "Dê um nome ao dispositivo (até 60 caracteres)."),
    ("Give the hours for every day.",
     "Indique o horário de todos os dias."),
    ("Mark appointments as done or missed one at a time.",
     "Marque como realizada ou falta uma marcação de cada vez."),
    ("Name at least one activity.",
     "Indique pelo menos uma atividade."),
    ("Not found.",
     "Não encontrado."),
    ("Only a recording that failed can be tried again.",
     "Só se pode tentar de novo uma gravação que falhou."),
    ("Only an applied change can be undone.",
     "Só se pode anular uma alteração aplicada."),
    ("Only group classes have attendees.",
     "Só as aulas de grupo têm inscritos."),
    ("Only monthly plans can be paused. End a package instead.",
     "Só os planos mensais podem ficar em pausa. Termine o pacote."),
    ("Only the owner can remove this document. You can remove documents you added today.",
     "Só o proprietário pode remover este documento. Pode remover documentos que juntou hoje."),
    ("Only the owner or whoever started this draft can change it.",
     "Só o proprietário ou quem começou o rascunho o pode alterar."),
    ("Only the owner or whoever started this draft can discard it.",
     "Só o proprietário ou quem começou este rascunho o pode descartar."),
    ("Part of the recording is missing. Send it again.",
     "Falta uma parte da gravação. Envie-a de novo."),
    ("Say why it’s being voided.",
     "Indique porque está a anular."),
    ("Send both the new start and the new end.",
     "Indique a nova hora de início e a nova hora de fim."),
    ("Sign in to continue.",
     "Entre para continuar."),
    ("Signed notes are kept. Add a new note to correct it.",
     "As notas assinadas são mantidas. Acrescente uma nota nova para corrigir."),
    ("Some details are too long.",
     "Alguns dados são demasiado longos."),
    ("Some details aren’t in the right format. Check dates, times and numbers.",
     "Alguns dados não estão no formato certo. Confirme datas, horas e números."),
    ("Some payments aren’t this patient’s or were voided. Refresh and try again.",
     "Alguns pagamentos não são deste utente."),
    ("Some required details are missing or not allowed.",
     "Faltam dados obrigatórios ou há dados não permitidos."),
    ("Some texts are too long. Shorten them and try again.",
     "Alguns textos são demasiado longos. Encurte-os e tente de novo."),
    ("Something this refers to doesn’t exist any more. Refresh and try again.",
     "Algo a que isto se refere já não existe. Atualize e tente de novo."),
    ("Something went wrong on the server.",
     "Algo correu mal no servidor."),
    ("That NIF isn’t valid. Check the 9 digits.",
     "Esse NIF não é válido. Confirme os 9 dígitos."),
    ("That appointment isn’t this patient’s.",
     "Essa marcação não é deste utente."),
    ("That code isn’t right for this email, or it has expired. Ask the clinic owner for a new one.",
     "Esse código não serve para este email, ou expirou. Peça um novo ao proprietário."),
    ("That code isn’t right. Check the time on your phone and try the newest code.",
     "Esse código não está certo. Confirme a hora do telemóvel e use o código mais recente."),
    ("That file couldn’t be read. Try again.",
     "Não foi possível ler o ficheiro. Tente outra vez."),
    ("That invite code isn’t valid or has expired. Ask the clinic owner for a new one.",
     "Esse código de convite não é válido ou expirou. Peça um novo ao proprietário."),
    ("That model name isn’t valid. Use a name like gemma3:12b.",
     "Esse nome de modelo não é válido. Use um nome como gemma3:12b."),
    ("That month is already closed.",
     "Esse mês já está fechado."),
    ("That month isn’t closed.",
     "Esse mês não está fechado."),
    ("That package belongs to another patient.",
     "Esse pacote pertence a outro utente."),
    ("That package can’t be used for this appointment (used up, expired, or another kind).",
     "Esse pacote não serve para esta marcação (esgotado, expirado ou de outro tipo)."),
    ("That package was cancelled.",
     "Esse pacote foi cancelado."),
    ("That password is too long.",
     "Essa palavra-passe é demasiado longa."),
    ("That recovery code isn’t right, or it was already used.",
     "Esse código de recuperação não está certo, ou já foi usado."),
    ("That time was booked for someone else meanwhile. Book it again by hand.",
     "Essa hora foi marcada para outra pessoa entretanto. Marque de novo à mão."),
    ("The AI can’t read HEIC photos. Use a JPEG or PDF (on an iPhone: Settings › Camera › Formats › Most Compatible), or type the details.",
     "A IA não lê fotos HEIC. Use JPEG ou PDF (no iPhone: Definições › Câmara › Formatos › Mais compatível), ou escreva os dados."),
    ("The AI couldn’t finish the draft. Try again, or write the letter yourself.",
     "A IA não conseguiu acabar o rascunho. Tente de novo, ou escreva a carta à mão."),
    ("The AI couldn’t finish the note. Try again, or keep your own text.",
     "A IA não conseguiu acabar a nota. Tente de novo, ou mantenha o seu texto."),
    ("The AI couldn’t finish the report. Try again; the figures are below.",
     "A IA não conseguiu acabar o relatório. Tente de novo; os números estão abaixo."),
    ("The AI couldn’t read this receipt. Try again, or type the details.",
     "A IA não conseguiu ler este talão. Tente de novo, ou escreva os dados."),
    ("The AI model isn’t installed on the server yet.",
     "O modelo de IA ainda não está instalado no servidor."),
    ("The appointment from this request has already been used, so it can’t be undone.",
     "A marcação deste pedido já foi usada, por isso não dá para desfazer."),
    ("The assistant didn’t understand that. Try saying it another way.",
     "O assistente não percebeu. Experimente dizer de outra forma."),
    ("The audio couldn’t be read.",
     "Não foi possível ler o áudio."),
    ("The backup service isn’t answering. Update the server, then try again.",
     "O serviço de cópias de segurança não está a responder. Atualize o servidor e tente de novo."),
    ("The chosen AI model can’t read pictures. Choose one that can (for example gemma3:12b or qwen2.5vl:7b) in Settings › Local AI, or type the details.",
     "O modelo de IA escolhido não lê imagens. Escolha um que leia (por exemplo gemma3:12b ou qwen2.5vl:7b) em Definições › IA local, ou escreva os dados."),
    ("The clinic is already set up. Sign in instead.",
     "A clínica já está configurada. Entre em vez disso."),
    ("The clinic owner removed this device. Ask them to approve it again in Settings › Devices.",
     "O responsável da clínica removeu este dispositivo. Peça-lhe que o aprove de novo em Definições › Dispositivos."),
    ("The day doesn’t match. Choose the day again.",
     "O dia não corresponde. Escolha o dia outra vez."),
    ("The draft isn’t ready yet.",
     "O rascunho ainda não está pronto."),
    ("The email or password is wrong.",
     "O email ou a palavra-passe estão errados."),
    ("The end must be after the start, on the same day.",
     "O fim tem de ser depois do início, no mesmo dia."),
    ("The file is missing from the clinic archive. Check the server’s archive folder or restore it from a backup.",
     "O ficheiro não está no arquivo da clínica. Verifique a pasta de arquivo do servidor ou reponha-o a partir de uma cópia de segurança."),
    ("The first day must be before the last day.",
     "O primeiro dia tem de ser antes do último."),
    ("The last day must be on or after the first day.",
     "O último dia tem de ser igual ou depois do primeiro."),
    ("The letter is too long.",
     "A carta é demasiado longa."),
    ("The logo is too big. Use a picture under 512 KB.",
     "O logótipo é demasiado grande. Use uma imagem com menos de 512 KB."),
    ("The logo must be a JPEG or PNG picture.",
     "O logótipo tem de ser uma imagem JPEG ou PNG."),
    ("The name doesn’t match. Type the patient’s name exactly as it appears in the file.",
     "O nome não corresponde. Escreva o nome do utente tal como aparece na ficha."),
    ("The owner always has the AI switch on.",
     "A IA está sempre ligada para a dona da clínica."),
    ("The payment date can’t be in the future.",
     "A data do pagamento não pode ser no futuro."),
    ("The recording hasn’t finished yet.",
     "A gravação ainda não terminou."),
    ("The recording is too short. Record the session, or delete this recording.",
     "A gravação é demasiado curta. Grave a sessão, ou apague esta gravação."),
    ("The report is empty.",
     "O relatório está vazio."),
    ("The score is higher than the test’s maximum.",
     "A pontuação é maior do que o máximo do teste."),
    ("The signature is missing. Ask the patient to sign again.",
     "Falta a assinatura. Peça ao utente para assinar de novo."),
    ("There’s already a patient with this NIF. Open that file instead.",
     "Já existe um utente com este NIF. Abra essa ficha em vez de criar outra."),
    ("There’s already a price for this kind of appointment. Change that one instead.",
     "Já existe um preço para este tipo de marcação. Altere esse."),
    ("This PDF is a scanned picture, so the AI can’t read it. Add a photo of the receipt instead, or type the details.",
     "Este PDF é uma digitalização, por isso a IA não o consegue ler. Anexe uma foto do talão, ou escreva os dados."),
    ("This PDF is part of the clinical record and can’t be changed.",
     "Este PDF faz parte do registo clínico e não pode ser alterado."),
    ("This PDF is part of the clinical record and is kept.",
     "Este PDF faz parte do registo clínico e é mantido."),
    ("This account is no longer active.",
     "Esta conta já não está ativa."),
    ("This already exists.",
     "Isto já existe."),
    ("This change was already applied or cancelled.",
     "Esta alteração já foi aplicada ou cancelada."),
    ("This class is full.",
     "A aula está cheia."),
    ("This consent text isn’t approved yet. The owner approves it in Settings › Consents.",
     "Este texto ainda não foi aprovado. O proprietário aprova-o em Definições › Consentimentos."),
    ("This consent was already withdrawn.",
     "Este consentimento já foi retirado."),
    ("This device was removed by the clinic owner.",
     "O responsável da clínica removeu este dispositivo."),
    ("This form has an answer it doesn’t know. Refresh and try again.",
     "Este formulário tem uma resposta desconhecida. Atualize e tente de novo."),
    ("This invite was already used or withdrawn.",
     "Este convite já foi usado ou retirado."),
    ("This letter is already approved.",
     "Esta carta já está aprovada."),
    ("This letter is approved and can’t be changed. Write a new letter instead.",
     "Esta carta está aprovada e não pode ser alterada."),
    ("This note is already signed or belongs to someone else.",
     "Esta nota já está assinada ou é de outra pessoa."),
    ("This note is signed or belongs to someone else, so it can’t be changed. Add a new note instead.",
     "Esta nota está assinada ou é de outra pessoa. Acrescente uma nota nova."),
    ("This overlaps another appointment for the same therapist.",
     "Sobrepõe-se a outra marcação do mesmo fisioterapeuta."),
    ("This package can’t be shared.",
     "Este pacote não pode ser partilhado."),
    ("This package has been used or paid, so it can’t be cancelled. End it instead.",
     "Este pacote já foi usado ou pago, por isso não pode ser anulado. Termine-o."),
    ("This package was cancelled.",
     "Este pacote foi cancelado."),
    ("This patient is already anonymised.",
     "Este utente já foi anonimizado."),
    ("This payment was already voided.",
     "Este pagamento já foi anulado."),
    ("This payment was voided.",
     "Este pagamento foi anulado."),
    ("This proposal is more than a day old. Ask the assistant again.",
     "Esta proposta tem mais de um dia. Peça de novo ao assistente."),
    ("This record is locked and can’t be changed.",
     "Este registo está bloqueado e não pode ser alterado."),
    ("This recording has already finished.",
     "Esta gravação já terminou."),
    ("This request was already answered.",
     "Este pedido já foi respondido."),
    ("This step expired. Sign in again.",
     "Este passo expirou. Entre de novo."),
    ("This text is already approved.",
     "Este texto já está aprovado."),
    ("This file is already in use (appointments, notes…), so it stays. Archive it in the patient’s file if needed.",
     "Esta ficha já está a ser usada (marcações, notas…), por isso fica. Se for preciso, arquive-a na ficha do utente."),
    ("This was changed again since, so it can’t be undone here. Change it by hand.",
     "Isto já foi alterado outra vez, por isso não pode ser anulado aqui. Altere-o à mão."),
    ("This was changed since the assistant suggested it. Ask the assistant again.",
     "Isto foi alterado depois da proposta do assistente. Peça de novo."),
    ("Those days have passed. Ask the assistant again.",
     "Esses dias já passaram. Peça de novo ao assistente."),
    ("Too many attempts. Try again in 15 minutes.",
     "Demasiadas tentativas. Tente de novo daqui a 15 minutos."),
    ("Too many questions left blank to work out a score.",
     "Há demasiadas perguntas em branco para calcular a pontuação."),
    ("Travel time must be between 0 and 240 minutes.",
     "A deslocação tem de ser entre 0 e 240 minutos."),
    ("Unknown kind of entry.",
     "Tipo de registo desconhecido."),
    ("Update the PhysioPulse app on this computer to sign in.",
     "Atualize a aplicação PhysioPulse neste computador para entrar."),
    ("Use Settings › Account for your own account.",
     "Para a sua própria conta, use Definições › Conta."),
    ("Use a PDF or a photo (JPEG, PNG, HEIC).",
     "Use um PDF ou uma fotografia (JPEG, PNG, HEIC)."),
    ("Use a rate between 0 and 100 %.",
     "Use uma taxa entre 0 e 100 %."),
    ("Use the address of a computer in the clinic, like 100.86.97.21 or 192.168.1.20:11434. Internet addresses aren’t allowed.",
     "Use o endereço de um computador da clínica, como 100.86.97.21 ou 192.168.1.20:11434. Endereços da internet não são permitidos."),
    ("Use the address of a computer in the clinic, like 192.168.1.66 or 100.86.97.21:8178. Internet addresses aren’t allowed.",
     "Use o endereço de um computador da clínica, como 192.168.1.66 ou 100.86.97.21:8178. Endereços da internet não são permitidos."),
    ("Use the email address the invite was sent to.",
     "Use o endereço de email para onde foi enviado o convite."),
    ("Write plain text, without < or >.",
     "Escreva só texto, sem < nem >."),
    ("Write the clinic’s name.",
     "Escreva o nome da clínica."),
    ("Write the letter first.",
     "Escreva a carta primeiro."),
    ("Write the message in both languages (up to 600 characters).",
     "Escreva a mensagem nas duas línguas (até 600 caracteres)."),
    ("Write the short clinical report.",
     "Escreva o relatório clínico."),
    ("Write what you’d like the assistant to do.",
     "Escreva o que quer que o assistente faça."),
    ("Write your name.",
     "Escreva o seu nome."),
    ("You can’t change your own role or deactivate yourself.",
     "Não pode mudar a sua própria função nem desativar-se."),
    ("Your current password isn’t right.",
     "A palavra-passe atual não está certa."),
    ("Your password isn’t right.",
     "A palavra-passe não está certa."),
    ("Your role can’t do this.",
     "A sua função não permite fazer isto."),
    ("Your session ended. Sign in again.",
     "A sua sessão terminou. Entre de novo."),
    ("You’re using this device. Remove it from another approved device.",
     "Está a usar este dispositivo. Remova-o a partir de outro dispositivo aprovado."),
    ("Less than 24 hours to go: call the clinic to cancel.",
     "Faltam menos de 24 horas: ligue para a clínica para cancelar."),
    ("Only reception, the patient’s therapist or the owner can do this.",
     "Só a receção, o fisioterapeuta do utente ou o proprietário podem fazer isto."),
    ("That account isn’t linked to this patient.",
     "Essa conta não está ligada a este utente."),
    ("This file was anonymised: it can’t take new contacts, documents or appointments.",
     "Esta ficha foi anonimizada: não pode receber novos contactos, documentos ou marcações."),
    ("Under 16s see their sessions through a parent’s or guardian’s account. Make a guardian invite.",
     "Os menores de 16 anos veem as sessões pela conta de um pai, mãe ou encarregado de educação. Crie um convite para o encarregado."),
    ("You already asked to move this session. The clinic will get back to you.",
     "Já pediu para mudar esta sessão. A clínica vai responder-lhe."),
    ("Some details aren’t right. Check them and send again.",
     "Alguns dados não estão certos. Verifique e envie de novo."),
    ("Choose a day from tomorrow and within the next two months.",
     "Escolha um dia a partir de amanhã e nos próximos dois meses."),
];

/// The Portuguese for a server message, if there is one.
pub fn pt(en: &str) -> Option<&'static str> {
    PT.iter().find(|(e, _)| *e == en).map(|(_, p)| *p)
}

/// Wraps every API answer: an error answered to a Portuguese app gets its `message` in Portuguese.
pub async fn localise(req: Request, next: Next) -> Response {
    let wants_pt = req.headers().get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.trim_start().to_ascii_lowercase().starts_with("pt"));
    let res = next.run(req).await;
    if !wants_pt || res.status().as_u16() < 400 {
        return res;
    }
    let is_json = res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"));
    if !is_json {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 1 << 20).await else {
        return Response::from_parts(parts, Body::empty());
    };
    let mut v: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return Response::from_parts(parts, Body::from(bytes)),
    };
    if let Some(p) = v.get("message").and_then(|m| m.as_str()).and_then(pt) {
        v["message"] = serde_json::Value::String(p.to_string());
    }
    let out = serde_json::to_vec(&v).unwrap_or_else(|_| bytes.to_vec());
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(out))
}

#[cfg(test)]
mod tests {
    use super::{pt, PT};

    /// Every literal message given to an `AppError` in the server's code has a Portuguese row.
    #[test]
    fn every_message_is_translated() {
        let mut missing = Vec::new();
        for path in glob(concat!(env!("CARGO_MANIFEST_DIR"), "/src")) {
            let src = std::fs::read_to_string(&path).unwrap();
            for m in messages(&src) {
                if pt(&m).is_none() {
                    missing.push(format!("{}: {m}", path.rsplit('/').next().unwrap()));
                }
            }
        }
        assert!(missing.is_empty(), "messages without Portuguese (add them to i18n::PT):\n{}", missing.join("\n"));
    }

    #[test]
    fn table_has_no_repeats() {
        let mut seen = std::collections::HashSet::new();
        for (e, p) in PT {
            assert!(seen.insert(*e), "repeated: {e}");
            assert!(!p.is_empty());
        }
    }

    fn glob(dir: &str) -> Vec<String> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(glob(p.to_str().unwrap()));
            } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("i18n.rs") {
                out.push(p.to_string_lossy().into_owned());
            }
        }
        out
    }

    /// The first string literal after `BadRequest(`, `Unauthorized(`, `Conflict(`, `ConflictWith("code",` and
    /// `ai_error(e, `, plus the fixed messages in error.rs.
    fn messages(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        for key in ["BadRequest(", "Unauthorized(", "Conflict(", "ConflictWith(", "DeviceRefused(", "ai_error(e, "] {
            let mut rest = src;
            while let Some(i) = rest.find(key) {
                rest = &rest[i + key.len()..];
                let mut t = rest.trim_start();
                if key == "ConflictWith(" || key == "DeviceRefused(" {
                    // skip the code: "ai_failed",
                    if let Some(after) = t.strip_prefix('"').and_then(|x| x.split_once('"')).map(|(_, r)| r.trim_start()) {
                        match after.strip_prefix(',') { Some(r) => t = r.trim_start(), None => continue }
                    }
                }
                if let Some(body) = t.strip_prefix('"') {
                    if let Some(end) = body.find('"') {
                        out.push(body[..end].to_string());
                    }
                }
            }
        }
        for fixed in ["Your role can’t do this.", "Not found.", "Too many attempts. Try again in 15 minutes.", "Something went wrong on the server."] {
            if src.contains(fixed) {
                out.push(fixed.to_string());
            }
        }
        out
    }
}
