# Avaliação de impacto sobre a proteção de dados (AIPD / DPIA) — RASCUNHO

**Rascunho escrito por Claude a 9 nov. 2026 para a dona da clínica rever.** Não é parecer jurídico. Preencha o que está entre [parênteses retos], corrija o que não corresponder à realidade e, se possível, peça a um jurista ou ao encarregado de proteção de dados que o leia uma vez. Depois de assinado, guarde uma cópia em PDF em `Clinica/Sistema/` e reveja-o todos os anos ou quando algo mudar (novo servidor, novo computador da IA, portal do utente, mais do que uma clínica).

Porquê: a lista da CNPD (Regulamento 1/2018) pede AIPD para tratamento de dados de saúde com novas tecnologias e transmissão eletrónica. Uma clínica pequena com servidor próprio é um caso limite; fazê-la é a opção segura (ver `LEGAL.md`).

## 1. Responsável e contexto
| | |
| --- | --- |
| Responsável pelo tratamento | [Nome da empresa], NIF [ ], [morada] |
| Registo ERS | [E…] |
| Contacto para proteção de dados | [nome, email] |
| Encarregado de proteção de dados (EPD) | Não obrigatório para uma clínica deste tamanho (art. 37.º RGPD: o tratamento não é "em grande escala"). [Confirmar.] |
| Data / versão | [data] · v1 |
| Aprovado por | [nome, assinatura] |

## 2. O que é tratado e para quê
| Finalidade | Dados | Fundamento (RGPD) | Quem acede |
| --- | --- | --- | --- |
| Prestar fisioterapia e manter o processo clínico | Identificação, contactos, NIF/SNS, anamnese, notas de evolução, testes, prescrições, fotos/vídeos clínicos, documentos, consentimentos | Art. 9.º, n.º 2, al. h) + Lei 58/2019, art. 29.º (profissionais sujeitos a sigilo) | Dona e fisioterapeutas (processo completo); receção só contactos, plano, consentimentos, prescrições (leitura) e pagamentos |
| Marcações e lembretes | Nome, contactos, horários | Execução do contrato / interesse legítimo | Toda a equipa |
| Faturação e contabilidade | Nome, NIF, pagamentos | Obrigação legal (CIVA, CIRC) | Dona; contabilista recebe a exportação mensal |
| Reclamações (Livro de Reclamações / ERS) | Reclamante, factos, resposta | Obrigação legal | Dona |
| Fotos/vídeos para ensino, redes ou site | Imagem | Consentimento específico (art. 9.º, n.º 2, al. a)) | Dona |
| Mensagens de marketing | Contactos | Consentimento (Lei 41/2004, art. 13.º-A) | Dona |

Titulares: utentes (incluindo menores, através do representante legal), representantes legais, equipa.

## 3. Como os dados circulam
- **Servidor da clínica** (ZimaOS, em casa da dona, [morada]): base de dados PostgreSQL e arquivo de ficheiros. Só acessível pela rede privada Tailscale; não está publicado na internet nem na rede local.
- **Computadores da clínica** (Mac e Windows) com a aplicação PhysioPulse: falam só com o servidor, por Tailscale (WireGuard, cifrado). A aplicação não guarda dados no computador; fecha a sessão ao fim de 15 minutos sem uso e ao fechar.
- **Computador da IA local** (Windows, 100.86.97.21, na clínica, na mesma rede Tailscale) com o Ollama e o modelo `gemma4:12b`. Recebe do servidor, só quando a dona pede: os factos de uma carta de alta, os campos de uma nota que está a escrever (sem nome), um talão de despesa, os números agregados de um mês, e — para o assistente (desde 13 nov. 2026, por decisão da dona) — o que a pessoa que pergunta pode ver na aplicação, só quando o assistente o consulta: a agenda, contactos, e, para a dona e fisioterapeutas, o processo clínico (anamnese, notas de evolução, testes, prescrições, consentimentos). A receção nunca recebe dados clínicos (o servidor aplica as mesmas regras da aplicação) e cada ficha consultada fica no registo de acessos. Nunca dados de pagamentos nesta fase. Não guarda pedidos nem respostas [confirmar: no Ollama, `OLLAMA_NOLOGS` / sem histórico]. O servidor recusa qualquer endereço de IA que não seja local ou Tailscale.
- **Cópias de segurança**: todas as noites, cifradas com uma chave pública `age` (o servidor não consegue ler as suas próprias cópias), guardadas 30 dias em [disco/pasta]; cópia externa [sim/não, onde].
- **Lembretes**: saem do WhatsApp, SMS ou email de quem está na clínica, com uma mensagem sem dados clínicos.
- **Contabilista**: recebe por [meio] o zip mensal (pagamentos, despesas, salários; sem dados clínicos).
- Nada é enviado para serviços de nuvem, IA na nuvem ou análise de utilização.

## 4. Necessidade e proporcionalidade
- Só se recolhe o necessário para tratar, faturar e cumprir a lei; campos opcionais ficam vazios.
- Prazos de conservação (`LEGAL.md`): processo clínico e consentimentos 20 anos após o último contacto (recomendação); faturas 10 anos; reclamações pelo menos 3 anos.
- Direitos dos titulares: cópia do processo (Exportar processo clínico), retificação (editar dados), apagamento/anonimização (ferramentas legais, com a ressalva dos prazos legais), oposição ao marketing (consentimento revogável). Respostas em 1 mês.
- A IA só é usada pela dona e por quem ela ligar o interruptor de IA em Equipa (a receção, se ligada, só usa o assistente para a agenda), como rascunho ou proposta; nada fica guardado sem ela ler e aceitar. Os textos e despesas que começaram na IA ficam marcados como tal.

## 5. Riscos e medidas
| Risco | Probabilidade · Gravidade | Medidas já tomadas | Risco que fica |
| --- | --- | --- | --- |
| Alguém de fora entra no servidor | Baixa · Alta | Sem portas na internet; Tailscale; contas pessoais; palavras-passe longas (Argon2id); 2 passos (TOTP) para todos; limite de tentativas; sessões de 15 min | Baixo. [Fazer: regra no Tailscale só para os computadores da clínica.] |
| Roubo ou perda do servidor ou do disco | Baixa · Alta | Cópias cifradas fora do servidor | **Médio**: o disco do servidor não está cifrado (decisão da dona, 7 nov. 2026). Rever se o servidor mudar de sítio |
| Roubo de um computador da clínica | Média · Média | A app não guarda dados; sessão fecha ao fechar | Baixo se o FileVault/BitLocker estiver ligado. [Fazer: ligar em todos.] |
| Roubo do computador da IA | Baixa · Média | Não guarda pedidos [confirmar]; recebe só o necessário | Baixo. [Fazer: BitLocker nesse PC.] |
| Funcionário vê o que não devia | Média · Média | Funções verificadas no servidor; receção sem dados clínicos; todas as aberturas de processo ficam no registo de acessos (não pode ser apagado) | Baixo; a dona revê o registo [mensalmente] |
| IA escreve algo errado num documento clínico | Média · Média | A IA só faz rascunhos; a dona lê, corrige e aprova; o texto fica marcado como vindo da IA | Baixo |
| Perda de dados (avaria, erro) | Média · Alta | Cópias noturnas cifradas; teste de reposição feito a 4 nov. 2026 | Baixo. [Fazer: repetir o teste por trimestre.] |
| Lembrete enviado para o número errado | Média · Baixa | Mensagem sem dados clínicos | Baixo |
| Dados enviados para fora da clínica (nuvem) | Baixa · Alta | Sem serviços de nuvem; IA só em endereços locais (verificado no servidor) | Baixo |

## 6. Conclusão
[Com as medidas acima, o risco residual é aceitável / não é aceitável.] Pontos a fechar antes de dados reais: regra de acesso no Tailscale, FileVault/BitLocker em todos os computadores (incluindo o da IA), confirmação de que o Ollama não guarda histórico, aviso de privacidade afixado e entregue, e esta avaliação assinada.

Não é necessária consulta prévia à CNPD (art. 36.º RGPD) se o risco residual for aceitável.

Assinatura: ______________________ Data: ___/___/______

## Addendum (11 Nov 2026, draft for the owner) — session recording
Processing: whole treatment sessions recorded in the clinic (patient asked first; no record of the answer kept — owner's accepted risk), transcribed by whisper.cpp on the clinic's AI PC and summarised by the local model into a draft note. Data: the voices of the patient, the therapist and any companion; health data said aloud. Storage: the clinic server only (`AUDIO_DIR`, outside backups), passing through the AI PC while transcribed; nothing leaves the clinic. Retention: deleted when the note is signed, or after 30 days. Access: the recording therapist and the owner; every open, play and deletion logged. Risk to review: third parties' voices in the recording; a companion should also be asked.

## Addendum (23 Nov 2026, draft for the owner) — patient area on the website
**What changes.** Patients the clinic invites can open an account on the clinic's website to see their upcoming and past sessions, send booking or "another time" requests, cancel a session up to 24 hours before, and download their invoice PDFs. A parent's or guardian's account can show a child under 16. Nothing clinical is shown (no notes, tests, prescriptions or clinical documents).
**New data.** Account email, a password hash (Argon2id), an optional two-step secret (encrypted), last sign-in, session hashes (deleted after 12 h at most), one-time code hashes, and the links between an account and the patients it shows. Requests and cancellations go into the existing booking-requests list.
**Where it lives.** The same clinic server. The website service still has no read access to the clinic's tables and gets a patient's own sessions and invoices only through the staff server, for a live session.
**Risks and measures.** *Someone else signs in as the patient* → invite and reset codes come only from the clinic, by the staff member's own WhatsApp/SMS; passwords of 10+ characters; rate limits; optional two-step; staff can remove access at once. *A shared family email* → guardian links are explicit and shown in the patient file. *A bug in the website* → it can't read the database; the worst case is limited to sessions, requests and invoices of patients with accounts. *Invoices on a phone* → downloaded only by the signed-in person; each download is logged.
**Retention.** Sessions: 12 h at most. Codes: until used or expired. Accounts: until access is removed, the patient is anonymised or deleted, or they ask.
**Residual risk:** low, given the scope (no clinical data). To be reviewed and signed by the owner before opening the area.
